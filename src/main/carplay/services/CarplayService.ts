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

  // One AudioOutput per (decodeType, audioType)
  private audioPlayers = new Map<PlayerKey, AudioOutput>()
  private lastStreamLogKey: PlayerKey | null = null

  // Logical per-stream volumes, controlled via IPC and config
  private volumes: VolumeState = {
    music: 1.0,
    nav: 0.5,
    siri: 0.5,
    call: 1.0
  }

  // Siri / phonecall state
  private siriActive = false
  private phonecallActive = false

  // Mute window after voice ends
  private readonly voiceTailMuteMs = 300
  private lastVoiceStopAt = 0

  // Soft ramp-in for music after voice tail
  private readonly musicRampInMs = 1000
  private musicFade: MusicFadeState = {
    current: 1,
    target: 1,
    remainingSamples: 0
  }

  constructor() {
    this.prewarmAudioPlayers()

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

  private prewarmAudioPlayers() {
    if (this.audioPlayers.size > 0) return

    const audioTypes = [1, 2, 3, 4]

    for (const [decodeTypeStr, meta] of Object.entries(decodeTypeMap)) {
      const decodeType = Number(decodeTypeStr) | 0
      const sampleRate = meta.frequency
      const channels = meta.channel

      for (const audioType of audioTypes) {
        const key: PlayerKey = `${decodeType}:${audioType}`
        if (this.audioPlayers.has(key)) continue

        const player = new AudioOutput({
          sampleRate,
          channels
        })
        player.start()
        this.audioPlayers.set(key, player)

        console.debug('[CarplayService] prewarmed AudioOutput', {
          playerKey: key,
          decodeType,
          audioType,
          sampleRate,
          channels
        })
      }
    }
  }

  private getAudioOutputForStream(msg: AudioData): AudioOutput | null {
    const audioType = msg.audioType ?? 1
    const streamKey: PlayerKey = `${msg.decodeType}:${audioType}`

    const meta = decodeTypeMap[msg.decodeType]
    if (!meta) {
      console.warn('[CarplayService] unknown decodeType in AudioData', {
        decodeType: msg.decodeType,
        audioType
      })
    }

    const player = this.audioPlayers.get(streamKey)
    if (!player) {
      console.warn('[CarplayService] no AudioOutput for streamKey', { streamKey })
      return null
    }

    if (this.lastStreamLogKey !== streamKey) {
      this.lastStreamLogKey = streamKey
      console.debug('[CarplayService] using AudioOutput for stream', {
        streamKey,
        decodeType: msg.decodeType,
        audioType,
        sampleRate: meta?.frequency,
        channels: meta?.channel
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

    // Downlink / output (music, nav, siri, phone, …)
    if (msg.data) {
      const now = Date.now()
      const voiceActive = this.siriActive || this.phonecallActive

      // Drop stray Siri/phone-coded frames if no voice session is active
      if (msg.decodeType === 5 && !voiceActive) {
        return
      }

      const player = this.getAudioOutputForStream(msg)
      if (!player) return

      const logicalKey = this.getLogicalStreamKey(msg)
      const baseGain = this.volumes[logicalKey] ?? 1.0

      let pcm: Int16Array

      if (logicalKey === 'music') {
        const sampleRate = meta?.frequency ?? 48000
        const channels = meta?.channel ?? 2
        const totalSamples = msg.data.length

        const inTailMute =
          this.lastVoiceStopAt > 0 && now - this.lastVoiceStopAt < this.voiceTailMuteMs

        const desiredFadeTarget = voiceActive || inTailMute ? 0 : 1

        if (desiredFadeTarget === 0) {
          // Immediate mute while voice is active
          this.musicFade.current = 0
          this.musicFade.target = 0
          this.musicFade.remainingSamples = 0

          pcm = new Int16Array(totalSamples)
        } else {
          // Play music again: ramp from 0 -> 1  (musicRampInMs)
          const fade = this.musicFade

          if (fade.target !== 1 || fade.remainingSamples === 0) {
            fade.target = 1
            fade.remainingSamples = Math.max(
              1,
              Math.round((this.musicRampInMs / 1000) * sampleRate * channels)
            )
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
        }
      } else {
        // nav / siri / call: no ramp
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

    // No PCM data: command-only messages (Siri / phonecall) -> mic and state
    if (msg.command != null) {
      console.debug('[CarplayService] audio command', { command: msg.command })

      if (
        msg.command === AudioCommand.AudioSiriStart ||
        msg.command === AudioCommand.AudioPhonecallStart
      ) {
        if (this.config.audioTransferMode) return

        if (msg.command === AudioCommand.AudioSiriStart) {
          this.siriActive = true
          this.phonecallActive = false
        } else if (msg.command === AudioCommand.AudioPhonecallStart) {
          this.phonecallActive = true
          this.siriActive = false
        }

        // Voice session: keep music fully muted, reset fade
        this.musicFade.current = 0
        this.musicFade.target = 0
        this.musicFade.remainingSamples = 0
        this.lastVoiceStopAt = 0

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
      } else if (
        msg.command === AudioCommand.AudioSiriStop ||
        msg.command === AudioCommand.AudioPhonecallStop
      ) {
        if (msg.command === AudioCommand.AudioSiriStop) {
          this.siriActive = false
        } else if (msg.command === AudioCommand.AudioPhonecallStop) {
          this.phonecallActive = false
        }
        this.lastVoiceStopAt = Date.now()
        this.musicFade.current = 0
        this.musicFade.target = 0
        this.musicFade.remainingSamples = 0

        this._mic?.stop()
      }
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
          const userConfig = JSON.parse(fs.readFileSync(configPath, 'utf8'))
          this.config = { ...this.config, ...userConfig }

          const ext = this.config as VolumeConfig
          this.volumes.music = typeof ext.audioVolume === 'number' ? ext.audioVolume : 1.0
          this.volumes.nav = typeof ext.navVolume === 'number' ? ext.navVolume : 0.5
          this.volumes.siri = typeof ext.siriVolume === 'number' ? ext.siriVolume : 0.5
          this.volumes.call = typeof ext.callVolume === 'number' ? ext.callVolume : 1.0

          console.debug('[CarplayService] initial volumes from config', {
            audioVolume: this.volumes.music,
            navVolume: this.volumes.nav,
            siriVolume: this.volumes.siri,
            callVolume: this.volumes.call
          })
        } catch {
          // defaults
        }

        this.siriActive = false
        this.phonecallActive = false
        this.lastVoiceStopAt = 0
        this.musicFade = { current: 1, target: 1, remainingSamples: 0 }

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

      this.started = false
      this.audioInfoSent = false
      this.firstFrameLogged = false
      this.lastStreamLogKey = null
      this.siriActive = false
      this.phonecallActive = false
      this.lastVoiceStopAt = 0
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
