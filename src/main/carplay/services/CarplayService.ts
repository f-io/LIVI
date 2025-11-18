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
  DongleDriver,
  DongleConfig,
  DEFAULT_CONFIG,
  decodeTypeMap,
  AudioCommand
} from '../messages'
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
type ConfigWithMediaSound = DongleConfig & {
  mediaSound?: 0 | 1
}

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
  private config: DongleConfig = DEFAULT_CONFIG
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

  // Logical per-stream volumes, controlled via IPC and config
  private volumes: VolumeState = {
    music: 1.0,
    nav: 0.5,
    siri: 1.0,
    call: 1.0
  }

  // Siri / phonecall state
  private siriActive = false
  private phonecallActive = false

  // Media session state (music)
  private mediaActive = false
  private audioOpenArmed = false

  // Delay and ramp configuration
  private readonly mediaDelaySafetyMs = 500
  private readonly musicRampInMs = 1000
  private readonly voiceTailMuteMs = 500

  // Fallback
  private readonly orphanMediaStartTimeoutMs = 2000
  private pendingMediaStartAt = 0

  // When to start the next ramp
  private nextMusicRampStartAt = 0
  private musicRampActive = false

  private musicFade: MusicFadeState = {
    current: 1,
    target: 1,
    remainingSamples: 0
  }

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

  // Prewarm one AudioOutput per (sampleRate, channels).
  private prewarmAudioPlayers() {
    if (this.audioPlayers.size > 0) return

    const mediaSound = (this.config as ConfigWithMediaSound).mediaSound ?? 1
    const preferredMediaSampleRate = mediaSound === 1 ? 48000 : 44100

    const seen = new Set<PlayerKey>()

    for (const [, meta] of Object.entries(decodeTypeMap)) {
      const sampleRate = meta.frequency
      const channels = meta.channel

      // For stereo media (music): only prewarm the preferred FS (44.1 or 48)
      if (channels === 2 && (sampleRate === 44100 || sampleRate === 48000)) {
        if (sampleRate !== preferredMediaSampleRate) {
          continue
        }
      }

      const key: PlayerKey = `${sampleRate}:${channels}`
      if (seen.has(key)) continue
      seen.add(key)

      const player = new AudioOutput({
        sampleRate,
        channels
      })
      player.start()
      this.audioPlayers.set(key, player)

      console.debug('[CarplayService] prewarmed AudioOutput', {
        playerKey: key,
        sampleRate,
        channels
      })
    }
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

    const player = this.audioPlayers.get(key)
    if (!player) {
      console.warn('[CarplayService] no AudioOutput for key', { key, sampleRate, channels })
      return null
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

      const player = this.getAudioOutputForStream(msg)
      if (!player) return

      const logicalKey = this.getLogicalStreamKey(msg)
      const volume = this.volumes[logicalKey] ?? 1.0

      // Map logical volume (0..1) to dB range (-60 dB .. 0 dB)
      const minDb = -60
      const maxDb = 0

      let baseGain: number
      if (volume <= 0) {
        baseGain = 0
      } else {
        const db = minDb + (maxDb - minDb) * volume
        baseGain = Math.pow(10, db / 20)
      }

      let pcm: Int16Array

      if (logicalKey === 'music') {
        const sampleRate = meta?.frequency ?? 48000
        const channels = meta?.channel ?? 2
        const totalSamples = msg.data.length

        // Fallback
        if (!this.mediaActive && this.pendingMediaStartAt > 0) {
          const elapsed = now - this.pendingMediaStartAt
          if (elapsed >= this.orphanMediaStartTimeoutMs) {
            this.mediaActive = true
            this.musicRampActive = true
            this.musicFade.current = 0
            this.musicFade.target = 1
            this.musicFade.remainingSamples = 0
            this.nextMusicRampStartAt = now
            this.pendingMediaStartAt = 0

            console.debug('[CarplayService] fallback AudioMediaStart after timeout', {
              elapsedMs: elapsed
            })
          }
        }

        // Always mute music if there is no active media session
        if (!this.mediaActive) {
          pcm = new Int16Array(totalSamples)
        } else if (voiceActive) {
          // Mute while Siri/phone is active
          pcm = new Int16Array(totalSamples)
        } else if (this.nextMusicRampStartAt > 0 && now < this.nextMusicRampStartAt) {
          // Still in delay window (mediaDelay + safety or voice tail)
          pcm = new Int16Array(totalSamples)
        } else if (!this.musicRampActive) {
          // Stable playback at full baseGain
          pcm = this.applyGain(msg.data, baseGain)
        } else {
          // Sample-based ramp from 0 -> 1 over musicRampInMs
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

            const g = baseGain * current
            let v = msg.data[i] * g
            if (v > 32767) v = 32767
            else if (v < -32768) v = -32768
            pcm[i] = v
          }

          fade.current = current
          fade.remainingSamples = remaining

          if (fade.remainingSamples === 0 || fade.current >= fade.target - 1e-3) {
            this.musicRampActive = false
          }
        }
      } else {
        // nav / siri / call: no ramp, volume mapping
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

    // Command-only messages: Siri / phone / media control
    if (msg.command != null) {
      const cmd = msg.command
      console.debug('[CarplayService] audio command', { command: cmd })

      // 1 == AudioOpen: arm genau ein nächstes AudioMediaStart
      if (cmd === 1) {
        this.audioOpenArmed = true
        this.mediaActive = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.pendingMediaStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        console.debug('[CarplayService] AudioOpen received, will accept next AudioMediaStart')
        return
      }

      if (cmd === AudioCommand.AudioMediaStart) {
        const baseDelay = this.getMediaDelay()
        const totalDelayMs = baseDelay + this.mediaDelaySafetyMs

        if (!this.audioOpenArmed) {
          if (!this.mediaActive && this.pendingMediaStartAt === 0) {
            this.pendingMediaStartAt = Date.now()
            console.debug('[CarplayService] AudioMediaStart pending (no preceding AudioOpen)', {
              mediaDelayMs: baseDelay,
              safetyMs: this.mediaDelaySafetyMs,
              totalDelayMs,
              fallbackAfterMs: this.orphanMediaStartTimeoutMs
            })
          } else {
            console.debug('[CarplayService] AudioMediaStart ignored (no preceding AudioOpen)', {
              mediaDelayMs: baseDelay,
              safetyMs: this.mediaDelaySafetyMs,
              totalDelayMs
            })
          }
          return
        }

        if (this.mediaActive) {
          console.debug('[CarplayService] AudioMediaStart ignored, media already active', {
            mediaDelayMs: baseDelay,
            safetyMs: this.mediaDelaySafetyMs,
            totalDelayMs
          })
          return
        }

        // Gültige 1→10-Sequenz
        this.audioOpenArmed = false
        this.mediaActive = true
        this.musicRampActive = true
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0
        this.pendingMediaStartAt = 0
        this.nextMusicRampStartAt = Date.now() + totalDelayMs

        console.debug('[CarplayService] AudioMediaStart received', {
          mediaDelayMs: baseDelay,
          safetyMs: this.mediaDelaySafetyMs,
          totalDelayMs
        })
        return
      }

      if (cmd === AudioCommand.AudioMediaStop) {
        this.mediaActive = false
        this.audioOpenArmed = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.pendingMediaStartAt = 0
        this.musicFade.current = 0
        this.musicFade.target = 1
        this.musicFade.remainingSamples = 0

        console.debug('[CarplayService] AudioMediaStop received, music muted')
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
        this.pendingMediaStartAt = 0
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
        } else if (cmd === AudioCommand.AudioPhonecallStop) {
          this.phonecallActive = false
        }

        // After voice: short tail mute + ramp-in for music, if media is active
        if (this.mediaActive) {
          this.musicRampActive = true
          this.musicFade.current = 0
          this.musicFade.target = 1
          this.musicFade.remainingSamples = 0
          this.nextMusicRampStartAt = Date.now() + this.voiceTailMuteMs
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
    const maybeWithMediaDelay = this.config as DongleConfig & { mediaDelay?: number }
    const raw = maybeWithMediaDelay.mediaDelay
    if (typeof raw === 'number' && Number.isFinite(raw) && raw >= 0) {
      return raw
    }
    return 0
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
          const userConfig = JSON.parse(fs.readFileSync(configPath, 'utf8'))
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
        this.mediaActive = false
        this.audioOpenArmed = false
        this.musicRampActive = false
        this.nextMusicRampStartAt = 0
        this.pendingMediaStartAt = 0
        this.musicFade = { current: 1, target: 1, remainingSamples: 0 }

        // Players depend on config (mediaSound), so prewarm now
        this.prewarmAudioPlayers()

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

      // Stop / clear all AudioOutput processes so no old samples survive
      this.stopAllAudioPlayers()

      this.started = false
      this.audioInfoSent = false
      this.firstFrameLogged = false
      this.lastStreamLogKey = null

      this.siriActive = false
      this.phonecallActive = false
      this.mediaActive = false
      this.audioOpenArmed = false
      this.musicRampActive = false
      this.nextMusicRampStartAt = 0
      this.pendingMediaStartAt = 0
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
