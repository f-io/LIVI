import { app, ipcMain, WebContents } from 'electron'
import { WebUSBDevice } from 'usb'
import {
  Plugged,
  Unplugged,
  VideoData,
  AudioData,
  MediaData,
  MediaType,
  Command,
  SendCommand,
  SendTouch,
  SendMultiTouch,
  SendAudio,
  SendFile,
  FileAddress,
  DongleDriver,
  DEFAULT_CONFIG,
  decodeTypeMap,
  AudioCommand
} from '../messages'
import { ExtraConfig } from '@main/Globals'
import fs from 'fs'
import path from 'path'
import usb from 'usb'
import { PersistedMediaPayload } from './types'
import { APP_START_TS, DEFAULT_MEDIA_DATA_RESPONSE } from './constants'
import { readMediaFile } from './utils/readMediaFile'
import { asDomUSBDevice } from './utils/asDomUSBDevice'
import { Microphone, AudioOutput, downsampleToMono } from '@audio'

let dongleConnected = false

type PlayerKey = string
type LogicalStreamKey = 'music' | 'nav' | 'siri' | 'call'

type VolumeConfig = {
  audioVolume?: number
  navVolume?: number
  siriVolume?: number
  callVolume?: number
}

type VolumeState = Record<LogicalStreamKey, number>

type MusicFadeState = {
  current: number
  target: number
  remainingSamples: number
}

export class CarplayService {
  private driver = new DongleDriver()
  private webUsbDevice: WebUSBDevice | null = null
  private webContents: WebContents | null = null
  private config: ExtraConfig = DEFAULT_CONFIG as ExtraConfig
  private pairTimeout: NodeJS.Timeout | null = null
  private frameInterval: NodeJS.Timeout | null = null
  private _mic: Microphone | null = null
  private started = false
  private stopping = false
  private shuttingDown = false
  private audioInfoSent = false
  private isStarting = false
  private startPromise: Promise<void> | null = null
  private isStopping = false
  private stopPromise: Promise<void> | null = null
  private firstFrameLogged = false

  // One AudioOutput per (sampleRate, channels)
  private audioPlayers = new Map<PlayerKey, AudioOutput>()
  private lastStreamLogKey: PlayerKey | null = null

  // Last used players per logical stream (for clean teardown)
  private lastMusicPlayerKey: PlayerKey | null = null
  private lastNavPlayerKey: PlayerKey | null = null
  private lastSiriPlayerKey: PlayerKey | null = null
  private lastCallPlayerKey: PlayerKey | null = null

  // Logical per-stream volumes, controlled via IPC and config
  private volumes: VolumeState = {
    music: 1.0,
    nav: 0.5,
    siri: 1.0,
    call: 1.0
  }

  // Siri / phonecall / nav state
  private siriActive = false
  private phonecallActive = false
  private navActive = false

  // Media session state (music)
  private mediaActive = false
  private audioOpenArmed = false

  // Ramp configuration
  private readonly musicRampInMs = 1000

  // When to start the next music ramp
  private nextMusicRampStartAt = 0
  private musicRampActive = false

  private musicFade: MusicFadeState = {
    current: 1,
    target: 1,
    remainingSamples: 0
  }

  // Queue for nav PCM that should be mixed into music
  private navMixQueue: Int16Array[] = []
  private navMixOffset = 0

  constructor() {
    this.driver.on('message', (msg) => {
      if (!this.webContents) return

      if (msg instanceof Plugged) {
        this.clearTimeouts()
        this.webContents.send('carplay-event', { type: 'plugged' })
        if (!this.started && !this.isStarting) {
          this.start().catch(() => {})
        }
      } else if (msg instanceof Unplugged) {
        this.webContents.send('carplay-event', { type: 'unplugged' })
        if (!this.shuttingDown && !this.stopping) {
          this.stop().catch(() => {})
        }
      } else if (msg instanceof VideoData) {
        if (!this.firstFrameLogged) {
          this.firstFrameLogged = true
          const dt = Date.now() - APP_START_TS
          console.log(`[Perf] AppStart→FirstFrame: ${dt} ms`)
        }
        this.webContents.send('carplay-event', {
          type: 'resolution',
          payload: { width: msg.width, height: msg.height }
        })
        this.sendChunked('carplay-video-chunk', msg.data?.buffer as ArrayBuffer, 512 * 1024)
      } else if (msg instanceof AudioData) {
        this.handleAudioData(msg)
      } else if (msg instanceof MediaData) {
        if (!msg.payload) return

        this.webContents!.send('carplay-event', { type: 'media', payload: msg })
        const file = path.join(app.getPath('userData'), 'mediaData.json')
        const existing = readMediaFile(file)
        const existingPayload = existing.payload
        const newPayload: PersistedMediaPayload = { type: msg.payload.type }

        if (msg.payload.type === MediaType.Data && msg.payload.media) {
          newPayload.media = { ...existingPayload.media, ...msg.payload.media }
          if (existingPayload.base64Image) newPayload.base64Image = existingPayload.base64Image
        } else if (msg.payload.type === MediaType.AlbumCover && msg.payload.base64Image) {
          newPayload.base64Image = msg.payload.base64Image
          if (existingPayload.media) newPayload.media = existingPayload.media
        } else {
          newPayload.media = existingPayload.media
          newPayload.base64Image = existingPayload.base64Image
        }
        const out = { timestamp: new Date().toISOString(), payload: newPayload }
        fs.writeFileSync(file, JSON.stringify(out, null, 2), 'utf8')
      } else if (msg instanceof Command) {
        this.webContents.send('carplay-event', { type: 'command', message: msg })
      }
    })

    this.driver.on('failure', () => {
      this.webContents?.send('carplay-event', { type: 'failure' })
    })

    ipcMain.handle('carplay-start', async () => this.start())
    ipcMain.handle('carplay-stop', async () => this.stop())
    ipcMain.handle('carplay-sendframe', async () => this.driver.send(new SendCommand('frame')))
    ipcMain.handle('carplay-upload-icons', async () => {
      if (!this.started || !this.webUsbDevice) {
        throw new Error('[CarplayService] CarPlay is not started or dongle not connected')
      }
      this.uploadIcons()
    })

    ipcMain.on('carplay-touch', (_evt, data: { x: number; y: number; action: number }) => {
      try {
        this.driver.send(new SendTouch(data.x, data.y, data.action))
      } catch {
        // ignore
      }
    })

    type MultiTouchPoint = { id: number; x: number; y: number; action: number }
    const to01 = (v: number): number => {
      const n = Number.isFinite(v) ? v : 0
      return n < 0 ? 0 : n > 1 ? 1 : n
    }
    const ONE_BASED_IDS = false

    ipcMain.on('carplay-multi-touch', (_evt, points: MultiTouchPoint[]) => {
      try {
        if (!Array.isArray(points) || points.length === 0) return
        const safe = points.map((p) => ({
          id: (p.id | 0) + (ONE_BASED_IDS ? 1 : 0),
          x: to01(p.x),
          y: to01(p.y),
          action: p.action | 0
        }))
        this.driver.send(new SendMultiTouch(safe))
      } catch {
        // ignore
      }
    })

    ipcMain.on('carplay-key-command', (_, command) => {
      this.driver.send(new SendCommand(command))
    })

    ipcMain.handle('carplay-media-read', async () => {
      try {
        const file = path.join(app.getPath('userData'), 'mediaData.json')

        if (!fs.existsSync(file)) {
          console.log('[carplay-media-read] Error: ENOENT: no such file or directory')
          return DEFAULT_MEDIA_DATA_RESPONSE
        }

        return readMediaFile(file)
      } catch (error) {
        console.log('[carplay-media-read]', error)
        return DEFAULT_MEDIA_DATA_RESPONSE
      }
    })

    ipcMain.on(
      'carplay-set-volume',
      (_evt, payload: { stream: LogicalStreamKey; volume: number }) => {
        const { stream, volume } = payload || {}
        this.setStreamVolume(stream, volume)
      }
    )
  }

  private stopAllAudioPlayers() {
    for (const player of this.audioPlayers.values()) {
      try {
        player.stop()
      } catch {
        // ignore
      }
    }
    this.audioPlayers.clear()
    this.lastStreamLogKey = null
    this.lastMusicPlayerKey = null
    this.lastNavPlayerKey = null
    this.lastSiriPlayerKey = null
    this.lastCallPlayerKey = null
  }

  private stopPlayerByKey(key: PlayerKey | null, label: string) {
    if (!key) return
    const player = this.audioPlayers.get(key)
    if (!player) return

    try {
      player.stop()
    } catch {
      // ignore
    }
    this.audioPlayers.delete(key)

    console.debug('[CarplayService] stopped AudioOutput', {
      label,
      playerKey: key
    })
  }

  private createAndStartAudioPlayer(sampleRate: number, channels: number): AudioOutput {
    const key: PlayerKey = `${sampleRate}:${channels}`

    const player = new AudioOutput({
      sampleRate,
      channels
    })
    player.start()
    this.audioPlayers.set(key, player)

    console.debug('[CarplayService] created AudioOutput', {
      playerKey: key,
      sampleRate,
      channels
    })

    return player
  }

  private getAudioOutputForStream(msg: AudioData): AudioOutput | null {
    const meta = decodeTypeMap[msg.decodeType]
    if (!meta) {
      console.warn('[CarplayService] unknown decodeType in AudioData', {
        decodeType: msg.decodeType,
        audioType: msg.audioType
      })
      return null
    }

    const sampleRate = meta.frequency
    const channels = meta.channel
    const key: PlayerKey = `${sampleRate}:${channels}`

    let player = this.audioPlayers.get(key)
    if (!player) {
      player = this.createAndStartAudioPlayer(sampleRate, channels)
    }

    if (this.lastStreamLogKey !== key) {
      this.lastStreamLogKey = key
      console.debug('[CarplayService] using AudioOutput for stream', {
        key,
        decodeType: msg.decodeType,
        audioType: msg.audioType,
        sampleRate,
        channels
      })
    }

    return player
  }

  private getLogicalStreamKey(msg: AudioData): LogicalStreamKey {
    const audioType = msg.audioType ?? 1

    if (audioType === 2) return 'nav'

    if (audioType === 1) {
      if (msg.decodeType === 4) {
        return 'music'
      }

      if (msg.decodeType === 5) {
        if (this.siriActive) return 'siri'
        if (this.phonecallActive) return 'call'
        return 'music'
      }

      if (this.siriActive) return 'siri'
      if (this.phonecallActive) return 'call'
      return 'music'
    }

    if (audioType === 3) return 'siri'
    if (audioType === 4) return 'call'

    return 'music'
  }

  private setStreamVolume(stream: LogicalStreamKey, volume: number) {
    if (!stream) return
    const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0))
    const prev = this.volumes[stream]

    if (prev !== undefined && Math.abs(prev - v) < 0.0001) {
      return
    }

    this.volumes[stream] = v
    console.debug('[CarplayService] setStreamVolume', { stream, volume: v })
  }

  private gainFromVolume(volume: number): number {
    const v = Math.max(0, Math.min(1, Number.isFinite(volume) ? volume : 0))
    if (v <= 0) return 0
    const minDb = -60
    const maxDb = 0
    const db = minDb + (maxDb - minDb) * v
    return Math.pow(10, db / 20)
  }

  private applyGain(pcm: Int16Array, gain: number): Int16Array {
    if (!Number.isFinite(gain) || gain === 1.0) {
      return pcm
    }
    if (gain <= 0) {
      return new Int16Array(pcm.length)
    }

    const out = new Int16Array(pcm.length)
    for (let i = 0; i < pcm.length; i += 1) {
      let v = pcm[i] * gain
      if (v > 32767) v = 32767
      else if (v < -32768) v = -32768
      out[i] = v
    }
    return out
  }

  private mixMusicAndNav(musicPcm: Int16Array, musicGain: number, navGain: number): Int16Array {
    if (navGain <= 0 || this.navMixQueue.length === 0) {
      // Nothing to mix, just apply music gain
      return this.applyGain(musicPcm, musicGain)
    }

    const out = new Int16Array(musicPcm.length)

    let navChunk = this.navMixQueue[0]
    let navOffset = this.navMixOffset

    for (let i = 0; i < musicPcm.length; i += 1) {
      const musicSample = musicPcm[i] * musicGain

      let navSample = 0
      if (navChunk) {
        navSample = navChunk[navOffset] * navGain
        navOffset += 1

        if (navOffset >= navChunk.length) {
          this.navMixQueue.shift()
          navChunk = this.navMixQueue[0] || null
          navOffset = 0
        }
      }

      let mixed = musicSample + navSample
      if (mixed > 32767) mixed = 32767
      else if (mixed < -32768) mixed = -32768

      out[i] = mixed
    }

    this.navMixOffset = navChunk ? navOffset : 0

    return out
  }

  private clearNavMix() {
    this.navMixQueue = []
    this.navMixOffset = 0
  }

  private handleAudioData(msg: AudioData) {
    const meta = decodeTypeMap[msg.decodeType]

    // PCM downlink / output (music, nav, siri, phone, …)
    if (msg.data) {
      const now = Date.now()
      const voiceActive = this.siriActive || this.phonecallActive

      // Drop Siri/phone-coded frames when no voice session is active
      if (msg.decodeType === 5 && !voiceActive) {
        return
      }

      const logicalKey = this.getLogicalStreamKey(msg)

      if (logicalKey === 'music' && !this.mediaActive) {
        return
      }

      const player = this.getAudioOutputForStream(msg)
      if (!player) return

      const volume = this.volumes[logicalKey] ?? 1.0

      // Track last player per logical stream for later teardown
      if (meta) {
        const keyForStream: PlayerKey = `${meta.frequency}:${meta.channel}`
        if (logicalKey === 'music') {
          this.lastMusicPlayerKey = keyForStream
        } else if (logicalKey === 'nav') {
          if (!this.mediaActive) this.lastNavPlayerKey = keyForStream
        } else if (logicalKey === 'siri') {
          this.lastSiriPlayerKey = keyForStream
        } else if (logicalKey === 'call') {
          this.lastCallPlayerKey = keyForStream
        }
      }

      const baseGain = this.gainFromVolume(volume)
      let pcm: Int16Array

      if (logicalKey === 'music') {
        const sampleRate = meta?.frequency ?? 48000
        const channels = meta?.channel ?? 2
        const totalSamples = msg.data.length

        // While Siri/phone is active, music is muted
        if (!this.mediaActive || voiceActive) {
          pcm = new Int16Array(totalSamples)
        } else if (this.nextMusicRampStartAt > 0 && now < this.nextMusicRampStartAt) {
          pcm = new Int16Array(totalSamples)
        } else if (!this.musicRampActive) {
          const navVolume = this.volumes.nav ?? 0.5
          const navGain = this.navActive ? this.gainFromVolume(navVolume) : 0

          if (this.navActive && this.navMixQueue.length > 0 && navGain > 0) {
            pcm = this.mixMusicAndNav(msg.data, baseGain, navGain)
          } else {
            pcm = this.applyGain(msg.data, baseGain)
          }
        } else {
          const fade = this.musicFade

          if (fade.remainingSamples === 0 && fade.current < fade.target) {
            fade.target = 1
            fade.remainingSamples = Math.max(
              1,
              Math.round((this.musicRampInMs / 1000) * sampleRate * channels)
            )
            console.debug('[CarplayService] starting music ramp', {
              samples: fade.remainingSamples,
              sampleRate,
              channels
            })
          }

          const navVolume = this.volumes.nav ?? 0.5
          const navGain = this.navActive ? this.gainFromVolume(navVolume) : 0

          pcm = new Int16Array(totalSamples)

          let current = fade.current
          let remaining = fade.remainingSamples
          const target = fade.target

          for (let i = 0; i < totalSamples; i += 1) {
            if (remaining > 0 && current < target) {
              const step = (target - current) / remaining
              current += step
              remaining -= 1
            } else {
              current = target
            }

            const musicSample = msg.data[i] * (baseGain * current)
            let navSample = 0

            if (this.navActive && this.navMixQueue.length > 0 && navGain > 0) {
              let navChunk = this.navMixQueue[0]
              let navOffset = this.navMixOffset

              if (navChunk) {
                navSample = navChunk[navOffset] * navGain
                navOffset += 1

                if (navOffset >= navChunk.length) {
                  this.navMixQueue.shift()
                  navChunk = this.navMixQueue[0] || null
                  navOffset = 0
                }
              }

              this.navMixOffset = navChunk ? navOffset : 0
            }

            let mixed = musicSample + navSample
            if (mixed > 32767) mixed = 32767
            else if (mixed < -32768) mixed = -32768

            pcm[i] = mixed
          }

          fade.current = current
          fade.remainingSamples = remaining

          if (fade.remainingSamples === 0 || fade.current >= fade.target - 1e-3) {
            this.musicRampActive = false
          }
        }
      } else if (logicalKey === 'nav') {
        // If music is active and nav is active, enqueue nav data for mixing
        if (this.mediaActive && this.navActive) {
          this.navMixQueue.push(msg.data.slice())
          return
        }

        // Nav-only playback (no music): just output nav
        pcm = this.applyGain(msg.data, baseGain)
      } else {
        // siri / call: no ramp, just volume mapping
        pcm = this.applyGain(msg.data, baseGain)
      }

      // Playback
      player.write(pcm)

      // Mono only for FFT visualization
      if (this.webContents && meta) {
        const inSampleRate = meta.frequency ?? 48000
        const inChannels = meta.channel ?? 2

        const mono = downsampleToMono(pcm, {
          inSampleRate,
          inChannels
        })

        if (mono.length > 0) {
          this.sendChunked('carplay-audio-chunk', mono.buffer as ArrayBuffer, 64 * 1024, {
            sampleRate: inSampleRate,
            channels: 1
          })
        }
      }

      if (!this.audioInfoSent && meta && this.webContents) {
        this.webContents.send('carplay-event', {
          type: 'audioInfo',
          payload: {
            codec: meta.format ?? meta.mimeType,
            sampleRate: meta.frequency,
            channels: meta.channel,
            bitDepth: meta.bitDepth
          }
        })
        this.audioInfoSent = true
      }

      return
    }

    // Command-only messages: Siri / phone / media / nav control
    if (msg.command != null) {
      const cmd = msg.command
      console.debug('[CarplayService] audio command', { command: cmd })

      // Explicit Nav / turn-by-turn start
      if (cmd === AudioCommand.AudioNaviStart || cmd === AudioCommand.AudioTurnByTurnStart) {
        this.navActive = true
        this.clearNavMix()
        console.debug('[CarplayService] Nav start received', { command: cmd })
        return
      }

      // 1 == AudioOpen: arm exactly one next AudioMediaStart
      if (cmd === AudioCommand.AudioOutputStart) {
        if (this.mediaActive) {
          console.debug('[CarplayService] AudioOpen ignored, media already active')
          return
        }

        this.audioOpenArmed = true
        this.mediaActive = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        console.debug('[CarplayService] AudioOpen received, will accept next AudioMediaStart')
        return
      }

      if (cmd === AudioCommand.AudioMediaStart) {
        const baseDelay = this.getMediaDelay()
        const totalDelayMs = baseDelay

        if (!this.audioOpenArmed) {
          if (!this.mediaActive) {
            // 10 without 1: treat as implicit open+start
            this.mediaActive = true
            this.musicRampActive = true
            this.musicFade.current = 0
            this.musicFade.target = 1
            this.musicFade.remainingSamples = 0
            this.nextMusicRampStartAt = Date.now() + totalDelayMs

            console.debug(
              '[CarplayService] AudioMediaStart without AudioOpen – treating as implicit open+start',
              {
                mediaDelayMs: baseDelay,
                totalDelayMs
              }
            )
          } else {
            console.debug(
              '[CarplayService] AudioMediaStart ignored (no preceding AudioOpen, media already active)',
              {
                mediaDelayMs: baseDelay,
                totalDelayMs
              }
            )
          }
          return
        }

        if (this.mediaActive) {
          console.debug('[CarplayService] AudioMediaStart ignored, media already active', {
            mediaDelayMs: baseDelay,
            totalDelayMs
          })
          return
        }

        this.audioOpenArmed = false
        this.mediaActive = true
        this.musicRampActive = true
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.nextMusicRampStartAt = Date.now() + totalDelayMs

        console.debug('[CarplayService] AudioMediaStart received', {
          mediaDelayMs: baseDelay,
          totalDelayMs
        })
        return
      }

      if (cmd === AudioCommand.AudioMediaStop) {
        this.mediaActive = false
        this.audioOpenArmed = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0

        if (this.lastMusicPlayerKey) {
          this.stopPlayerByKey(this.lastMusicPlayerKey, 'music')
          this.lastMusicPlayerKey = null
        }

        console.debug('[CarplayService] AudioMediaStop received, music muted')
        return
      }

      if (cmd === AudioCommand.AudioNaviStop || cmd === AudioCommand.AudioTurnByTurnStop) {
        this.navActive = false
        if (!this.mediaActive && this.lastNavPlayerKey) {
          this.stopPlayerByKey(this.lastNavPlayerKey, 'nav')
          this.lastNavPlayerKey = null
        } else {
          // mixing with music -> do not kill shared player, let tail drain
        }
        console.debug('[CarplayService] Nav stop received', {
          command: cmd,
          killed: !this.mediaActive
        })
        return
      }

      if (cmd === AudioCommand.AudioSiriStart || cmd === AudioCommand.AudioPhonecallStart) {
        if (this.config.audioTransferMode) return

        if (cmd === AudioCommand.AudioSiriStart) {
          this.siriActive = true
          this.phonecallActive = false
        } else if (cmd === AudioCommand.AudioPhonecallStart) {
          this.phonecallActive = true
          this.siriActive = false
        }

        // While voice is active, keep music muted
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0

        if (!this._mic) {
          this._mic = new Microphone()

          this._mic.on('data', (data: Buffer) => {
            if (!data || data.byteLength === 0) return

            const pcm16 = new Int16Array(data.buffer)

            try {
              this.driver.send(new SendAudio(pcm16))
            } catch (e) {
              console.error('[CarplayService] failed to send mic audio', e)
            }
          })
        }

        this._mic.start()
        return
      }

      if (cmd === AudioCommand.AudioSiriStop || cmd === AudioCommand.AudioPhonecallStop) {
        if (cmd === AudioCommand.AudioSiriStop) {
          this.siriActive = false
          if (this.lastSiriPlayerKey) {
            this.stopPlayerByKey(this.lastSiriPlayerKey, 'siri')
            this.lastSiriPlayerKey = null
          }
        } else if (cmd === AudioCommand.AudioPhonecallStop) {
          this.phonecallActive = false
          if (this.lastCallPlayerKey) {
            this.stopPlayerByKey(this.lastCallPlayerKey, 'call')
            this.lastCallPlayerKey = null
          }
        }

        // After voice: ramp-in for music, if media is active
        if (this.mediaActive) {
          this.musicRampActive = true
          this.musicFade.current = 0
          this.musicFade.target = 1
          this.musicFade.remainingSamples = 0
          this.nextMusicRampStartAt = Date.now()
        } else {
          this.musicRampActive = false
          this.nextMusicRampStartAt = 0
          this.musicFade.current = 0
          this.musicFade.target = 1
          this.musicFade.remainingSamples = 0
        }

        this._mic?.stop()
        return
      }
    }
  }

  private getMediaDelay(): number {
    const raw = this.config.mediaDelay
    return typeof raw === 'number' && Number.isFinite(raw) && raw >= 0 ? raw : 0
  }

  private uploadIcons() {
    try {
      const configPath = path.join(app.getPath('userData'), 'config.json')

      let cfg: ExtraConfig = { ...(DEFAULT_CONFIG as ExtraConfig), ...this.config }

      try {
        if (fs.existsSync(configPath)) {
          const diskCfg = JSON.parse(fs.readFileSync(configPath, 'utf8')) as ExtraConfig
          cfg = { ...cfg, ...diskCfg }
          this.config = cfg
        }
      } catch (err) {
        console.warn(
          '[CarplayService] failed to reload config.json before icon upload, using in-memory config',
          err
        )
      }

      const b120 = cfg.dongleIcon120 ? cfg.dongleIcon120.trim() : ''
      const b180 = cfg.dongleIcon180 ? cfg.dongleIcon180.trim() : ''
      const b256 = cfg.dongleIcon256 ? cfg.dongleIcon256.trim() : ''

      if (!b120 || !b180 || !b256) {
        console.error('[CarplayService] Icon fields missing in config.json — upload cancelled')
        return
      }

      const buf120 = Buffer.from(b120, 'base64')
      const buf180 = Buffer.from(b180, 'base64')
      const buf256 = Buffer.from(b256, 'base64')

      this.driver.send(new SendFile(buf120, FileAddress.ICON_120))
      this.driver.send(new SendFile(buf180, FileAddress.ICON_180))
      this.driver.send(new SendFile(buf256, FileAddress.ICON_256))

      console.debug('[CarplayService] uploaded icons from fresh config.json')
    } catch (err) {
      console.error('[CarplayService] failed to upload icons', err)
    }
  }

  public attachRenderer(webContents: WebContents) {
    this.webContents = webContents
  }

  public markDongleConnected(connected: boolean) {
    dongleConnected = connected
  }

  public async autoStartIfNeeded() {
    if (this.shuttingDown) return
    if (!this.started && !this.isStarting && dongleConnected) {
      await this.start()
    }
  }

  private async start() {
    if (this.started) return
    if (this.isStarting) return this.startPromise ?? Promise.resolve()

    this.isStarting = true
    this.startPromise = (async () => {
      try {
        const configPath = path.join(app.getPath('userData'), 'config.json')
        try {
          const userConfig = JSON.parse(fs.readFileSync(configPath, 'utf8')) as ExtraConfig
          this.config = { ...this.config, ...userConfig }

          const ext = this.config as VolumeConfig
          this.volumes.music = typeof ext.audioVolume === 'number' ? ext.audioVolume : 1.0
          this.volumes.nav = typeof ext.navVolume === 'number' ? ext.navVolume : 0.5
          this.volumes.siri = typeof ext.siriVolume === 'number' ? ext.siriVolume : 1.0
          this.volumes.call = typeof ext.callVolume === 'number' ? ext.callVolume : 1.0

          console.debug('[CarplayService] initial volumes from config', {
            audioVolume: this.volumes.music,
            navVolume: this.volumes.nav,
            siriVolume: this.volumes.siri,
            callVolume: this.volumes.call,
            mediaDelay: this.getMediaDelay()
          })
        } catch {
          // defaults
        }

        // Reset audio state
        this.siriActive = false
        this.phonecallActive = false
        this.navActive = false
        this.mediaActive = false
        this.audioOpenArmed = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.musicFade = { current: 1, target: 1, remainingSamples: 0 }
        this.lastMusicPlayerKey = null
        this.lastNavPlayerKey = null
        this.lastSiriPlayerKey = null
        this.lastCallPlayerKey = null
        this.clearNavMix()

        const device = usb
          .getDeviceList()
          .find(
            (d) =>
              d.deviceDescriptor.idVendor === 0x1314 &&
              [0x1520, 0x1521].includes(d.deviceDescriptor.idProduct)
          )
        if (!device) return

        try {
          const webUsbDevice = await WebUSBDevice.createInstance(device)
          await webUsbDevice.open()
          this.webUsbDevice = webUsbDevice

          await this.driver.initialise(asDomUSBDevice(webUsbDevice))
          await this.driver.start(this.config)

          this.pairTimeout = setTimeout(() => {
            this.driver.send(new SendCommand('wifiPair'))
          }, 15000)

          this.started = true
          this.audioInfoSent = false
          this.firstFrameLogged = false
          this.lastStreamLogKey = null
          this.lastMusicPlayerKey = null
          this.lastNavPlayerKey = null
          this.lastSiriPlayerKey = null
          this.lastCallPlayerKey = null
        } catch {
          try {
            await this.webUsbDevice?.close()
          } catch {}
          this.webUsbDevice = null
          this.started = false
        }
      } finally {
        this.isStarting = false
        this.startPromise = null
      }
    })()

    return this.startPromise
  }

  public async stop(): Promise<void> {
    if (this.isStopping) return this.stopPromise ?? Promise.resolve()
    if (!this.started || this.stopping) return

    this.stopping = true
    this.isStopping = true
    this.stopPromise = (async () => {
      this.clearTimeouts()
      try {
        await this.driver.close()
      } catch {}
      try {
        this._mic?.stop()
      } catch {}
      try {
        await this.webUsbDevice?.close()
      } catch {
      } finally {
        this.webUsbDevice = null
      }

      this.stopAllAudioPlayers()
      this.clearNavMix()

      this.started = false
      this.audioInfoSent = false
      this.firstFrameLogged = false
      this.lastStreamLogKey = null
      this.lastMusicPlayerKey = null
      this.lastNavPlayerKey = null
      this.lastSiriPlayerKey = null
      this.lastCallPlayerKey = null

      this.siriActive = false
      this.phonecallActive = false
      this.navActive = false
      this.mediaActive = false
      this.audioOpenArmed = false
      this.musicRampActive = false
      this.nextMusicRampStartAt = 0
      this.musicFade = { current: 1, target: 1, remainingSamples: 0 }
    })().finally(() => {
      this.stopping = false
      this.isStopping = false
      this.stopPromise = null
    })

    return this.stopPromise
  }

  private clearTimeouts() {
    if (this.pairTimeout) {
      clearTimeout(this.pairTimeout)
      this.pairTimeout = null
    }
    if (this.frameInterval) {
      clearInterval(this.frameInterval)
      this.frameInterval = null
    }
  }

  private sendChunked(
    channel: string,
    data?: ArrayBuffer,
    chunkSize = 512 * 1024,
    extra?: Record<string, unknown>
  ) {
    if (!this.webContents || !data) return
    let offset = 0
    const total = data.byteLength
    const id = Math.random().toString(36).slice(2)

    while (offset < total) {
      const end = Math.min(offset + chunkSize, total)
      const chunk = data.slice(offset, end)

      const envelope: {
        id: string
        offset: number
        total: number
        isLast: boolean
        chunk: Buffer
      } & Record<string, unknown> = {
        id,
        offset,
        total,
        isLast: end >= total,
        chunk: Buffer.from(chunk),
        ...(extra ?? {})
      }

      this.webContents.send(channel, envelope)
      offset = end
    }
  }
}
