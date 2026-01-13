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
  BoxInfo,
  SoftwareVersion,
  SendCommand,
  SendTouch,
  SendMultiTouch,
  SendAudio,
  SendFile,
  SendDisconnectPhone,
  SendCloseDongle,
  FileAddress,
  DongleDriver,
  DEFAULT_CONFIG
} from '../messages'
import { ExtraConfig } from '@main/Globals'
import fs from 'fs'
import path from 'path'
import usb from 'usb'
import { PersistedMediaPayload } from './types'
import { APP_START_TS, DEFAULT_MEDIA_DATA_RESPONSE } from './constants'
import { readMediaFile } from './utils/readMediaFile'
import { asDomUSBDevice } from './utils/asDomUSBDevice'
import { CarplayAudio, LogicalStreamKey } from './CarplayAudio'

let dongleConnected = false

type VolumeConfig = {
  audioVolume?: number
  navVolume?: number
  siriVolume?: number
  callVolume?: number
}

export class CarplayService {
  private driver = new DongleDriver()
  private webUsbDevice: WebUSBDevice | null = null
  private webContents: WebContents | null = null
  private config: ExtraConfig = DEFAULT_CONFIG as ExtraConfig
  private pairTimeout: NodeJS.Timeout | null = null
  private frameInterval: NodeJS.Timeout | null = null

  private started = false
  private stopping = false
  private shuttingDown = false
  private isStarting = false
  private startPromise: Promise<void> | null = null
  private isStopping = false
  private stopPromise: Promise<void> | null = null
  private firstFrameLogged = false
  private lastVideoWidth?: number
  private lastVideoHeight?: number
  private dongleFwVersion?: string
  private boxInfo?: unknown
  private lastDongleInfoEmitKey = ''

  private audio: CarplayAudio

  constructor() {
    this.audio = new CarplayAudio(
      () => this.config,
      (payload) => {
        this.webContents?.send('carplay-event', payload)
      },
      (channel, data, chunkSize, extra) => {
        this.sendChunked(channel, data, chunkSize, extra)
      },
      (pcm) => {
        try {
          this.driver.send(new SendAudio(pcm))
        } catch (e) {
          console.error('[CarplayService] failed to send mic audio', e)
        }
      }
    )

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
        const w = msg.width
        const h = msg.height
        if (w > 0 && h > 0 && (w !== this.lastVideoWidth || h !== this.lastVideoHeight)) {
          this.lastVideoWidth = w
          this.lastVideoHeight = h

          this.webContents.send('carplay-event', {
            type: 'resolution',
            payload: { width: w, height: h }
          })
        }

        this.sendChunked('carplay-video-chunk', msg.data?.buffer as ArrayBuffer, 512 * 1024)
      } else if (msg instanceof AudioData) {
        this.audio.handleAudioData(msg)

        // Forward command-only AudioData messages to renderer so UI can react
        // (call ringing, siri start/stop, nav start/stop, etc)
        if (msg.command != null) {
          this.webContents.send('carplay-event', {
            type: 'audio',
            payload: {
              command: msg.command,
              audioType: msg.audioType,
              decodeType: msg.decodeType,
              volume: msg.volume
            }
          })
        }
      } else if (msg instanceof MediaData) {
        if (!msg.payload) return

        this.webContents.send('carplay-event', { type: 'media', payload: msg })
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
      } else if (msg instanceof SoftwareVersion) {
        this.dongleFwVersion = msg.version
        this.emitDongleInfoIfChanged()
      } else if (msg instanceof BoxInfo) {
        this.boxInfo = msg.settings
        this.emitDongleInfoIfChanged()
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
        this.audio.setStreamVolume(stream, volume)
      }
    )

    // visualizer / FFT toggle from renderer
    ipcMain.on('carplay-set-visualizer-enabled', (_evt, enabled: boolean) => {
      this.audio.setVisualizerEnabled(Boolean(enabled))
    })
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

  private emitDongleInfoIfChanged() {
    if (!this.webContents) return

    let boxKey = ''
    if (this.boxInfo != null) {
      try {
        boxKey = JSON.stringify(this.boxInfo)
      } catch {
        boxKey = String(this.boxInfo)
      }
    }

    const key = `${this.dongleFwVersion ?? ''}||${boxKey}`
    if (key === this.lastDongleInfoEmitKey) return
    this.lastDongleInfoEmitKey = key

    this.webContents.send('carplay-event', {
      type: 'dongleInfo',
      payload: {
        dongleFwVersion: this.dongleFwVersion,
        boxInfo: this.boxInfo
      }
    })
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

          this.audio.setInitialVolumes({
            music: typeof ext.audioVolume === 'number' ? ext.audioVolume : undefined,
            nav: typeof ext.navVolume === 'number' ? ext.navVolume : undefined,
            siri: typeof ext.siriVolume === 'number' ? ext.siriVolume : undefined,
            call: typeof ext.callVolume === 'number' ? ext.callVolume : undefined
          })
        } catch {
          // defaults
        }

        this.audio.resetForSessionStart()

        this.dongleFwVersion = undefined
        this.boxInfo = undefined
        this.lastDongleInfoEmitKey = ''
        this.lastVideoWidth = undefined
        this.lastVideoHeight = undefined

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

  public async disconnectPhone(): Promise<boolean> {
    if (!this.started) return false

    let ok = false
    try {
      ok = (await this.driver.send(new SendDisconnectPhone())) || ok
    } catch (e) {
      console.warn('[CarplayService] SendDisconnectPhone failed', e)
    }

    try {
      ok = (await this.driver.send(new SendCloseDongle())) || ok
    } catch (e) {
      console.warn('[CarplayService] SendCloseDongle failed', e)
    }

    if (ok) await new Promise((r) => setTimeout(r, 150))

    return ok
  }

  public async stop(): Promise<void> {
    if (this.isStopping) return this.stopPromise ?? Promise.resolve()
    if (!this.started || this.stopping) return

    this.stopping = true
    this.isStopping = true

    this.stopPromise = (async () => {
      this.clearTimeouts()

      try {
        await this.disconnectPhone()
      } catch {}

      try {
        if (process.platform === 'darwin' && this.webUsbDevice) {
          await this.webUsbDevice.reset()
        }
      } catch (e) {
        console.warn('[CarplayService] webUsbDevice.reset() failed (ignored)', e)
      }

      try {
        await this.driver.close()
      } catch (e) {
        console.warn('[CarplayService] driver.close() failed (ignored)', e)
      }

      this.webUsbDevice = null
      this.audio.resetForSessionStop()

      this.started = false

      this.dongleFwVersion = undefined
      this.boxInfo = undefined
      this.lastDongleInfoEmitKey = ''
      this.lastVideoWidth = undefined
      this.lastVideoHeight = undefined
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
