import { app, WebContents } from 'electron'
import { WebUSBDevice, usb } from 'usb'
import {
  Plugged,
  Unplugged,
  PhoneType,
  BluetoothPairedList,
  VideoData,
  AudioData,
  MetaData,
  MediaType,
  Command,
  BoxInfo,
  SoftwareVersion,
  SendRawMessage,
  GnssData,
  SendCommand,
  SendTouch,
  SendMultiTouch,
  SendAudio,
  SendFile,
  SendServerCgiScript,
  SendLiviWeb,
  SendDisconnectPhone,
  SendCloseDongle,
  FileAddress,
  DongleDriver,
  BoxUpdateProgress,
  BoxUpdateState,
  MessageType,
  decodeTypeMap,
  DEFAULT_CONFIG
} from '../messages'
import { PhoneWorkMode } from '@shared/types'
import type { ExtraConfig, DongleFirmwareAction, DongleFwApiRaw } from '@shared/types'
import fs from 'fs'
import path from 'path'
import { PersistedMediaPayload, PersistedNavigationPayload } from './types'
import {
  APP_START_TS,
  DEFAULT_MEDIA_DATA_RESPONSE,
  DEFAULT_NAVIGATION_DATA_RESPONSE
} from './constants'
import { readMediaFile } from './utils/readMediaFile'
import { readNavigationFile } from './utils/readNavigationFile'
import { normalizeNavigationPayload } from './utils/normalizeNavigation'
import { translateNavigation } from '@shared/utils'
import type { NavLocale } from '@shared/utils'
import { asDomUSBDevice } from './utils/asDomUSBDevice'
import { ProjectionAudio, LogicalStreamKey } from './ProjectionAudio'
import { FirmwareUpdateService, FirmwareCheckResult } from './FirmwareUpdateService'
import { configEvents } from '@main/ipc/utils'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'

let dongleConnected = false

type VolumeConfig = {
  audioVolume?: number
  navVolume?: number
  siriVolume?: number
  callVolume?: number
}

type DongleFirmwareRequest = { action: DongleFirmwareAction }

type DongleFwCheckResponse = {
  ok: boolean
  hasUpdate: boolean
  size: string | number
  token?: string
  request?: Record<string, unknown>
  raw: DongleFwApiRaw
  error?: string
}

type DevToolsUploadResult = {
  ok: boolean
  cgiOk: boolean
  webOk: boolean
  urls: string[]
  startedAt: string
  finishedAt: string
  durationMs: number
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

function pickString(o: Record<string, unknown>, key: string): string | undefined {
  const v = o[key]
  return typeof v === 'string' ? v : undefined
}

function pickNumber(o: Record<string, unknown>, key: string): number | undefined {
  const v = o[key]
  return typeof v === 'number' ? v : undefined
}

function pickStringOrNumber(o: Record<string, unknown>, key: string): string | number | undefined {
  const v = o[key]
  return typeof v === 'string' || typeof v === 'number' ? v : undefined
}

export class ProjectionService {
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
  private lastAudioMetaEmitKey = ''
  private firmware = new FirmwareUpdateService()

  private lastNaviVideoWidth?: number
  private lastNaviVideoHeight?: number
  private mapsRequested = false
  private lastPluggedPhoneType?: PhoneType
  private aaPlaybackInferred: 1 | 2 = 1

  private audio: ProjectionAudio

  private readonly onConfigChanged = (next: ExtraConfig) => {
    if (this.shuttingDown) return
    this.config = { ...this.config, ...next }
  }

  private subscribeConfigEvents(): void {
    configEvents.on('changed', this.onConfigChanged)
  }

  private unsubscribeConfigEvents(): void {
    configEvents.off('changed', this.onConfigChanged)
  }

  public beginShutdown(): void {
    this.shuttingDown = true
    this.unsubscribeConfigEvents()
  }

  constructor() {
    this.audio = new ProjectionAudio(
      () => this.config,
      (payload) => {
        this.webContents?.send('projection-event', payload)
      },
      (channel, data, chunkSize, extra) => {
        this.sendChunked(channel, data, chunkSize, extra)
      },
      (pcm, decodeType) => {
        try {
          this.driver.send(new SendAudio(pcm, decodeType))
        } catch (e) {
          console.error('[ProjectionService] failed to send mic audio', e)
        }
      }
    )

    this.driver.on('message', (msg) => {
      // Always keep updater-relevant state, even if renderer is not attached yet.
      if (msg instanceof SoftwareVersion) {
        this.dongleFwVersion = msg.version
        this.emitDongleInfoIfChanged()
        return
      }

      if (msg instanceof BoxInfo) {
        this.boxInfo = mergePreferExisting(this.boxInfo, msg.settings)
        this.emitDongleInfoIfChanged()
        return
      }

      if (msg instanceof GnssData) {
        this.webContents?.send('projection-event', {
          type: 'gnss',
          payload: {
            text: msg.text
          }
        })
        return
      }

      if (!this.webContents) return

      if (msg instanceof BluetoothPairedList) {
        this.webContents.send('projection-event', {
          type: 'bluetoothPairedList',
          payload: msg.data
        })
        return
      }

      if (msg instanceof Plugged) {
        this.clearTimeouts()
        this.lastPluggedPhoneType = msg.phoneType
        this.aaPlaybackInferred = 1

        const nextPhoneWorkMode =
          msg.phoneType === PhoneType.CarPlay ? PhoneWorkMode.CarPlay : PhoneWorkMode.Android

        try {
          configEvents.emit('requestSave', { lastPhoneWorkMode: nextPhoneWorkMode })
        } catch (e) {
          console.warn('[ProjectionService] failed to persist lastPhoneWorkMode (ignored)', e)
        }

        const phoneTypeConfig = this.config.phoneConfig?.[msg.phoneType]
        if (phoneTypeConfig?.frameInterval) {
          this.frameInterval = setInterval(() => {
            if (!this.started) return
            try {
              this.driver.send(new SendCommand('frame'))
            } catch {}
          }, phoneTypeConfig.frameInterval)
        }
        this.webContents.send('projection-event', { type: 'plugged' })
        if (!this.started && !this.isStarting) {
          this.start().catch(() => {})
        }
      } else if (msg instanceof Unplugged) {
        this.clearTimeouts()
        this.lastPluggedPhoneType = undefined
        this.aaPlaybackInferred = 1
        this.webContents.send('projection-event', { type: 'unplugged' })
        this.resetNavigationSnapshot('unplugged')

        if (!this.shuttingDown && !this.stopping) {
          this.stop().catch(() => {})
        }
      } else if (msg instanceof BoxUpdateProgress) {
        // 0xb1 payload: int32 progress
        this.webContents.send('projection-event', {
          type: 'fwUpdate',
          stage: 'upload:progress',
          progress: msg.progress
        })
      } else if (msg instanceof BoxUpdateState) {
        // 0xbb payload: int32 status (start/success/fail, ota variants)
        this.webContents.send('projection-event', {
          type: 'fwUpdate',
          stage: 'upload:state',
          status: msg.status,
          statusText: msg.statusText,
          isOta: msg.isOta,
          isTerminal: msg.isTerminal,
          ok: msg.ok
        })

        if (msg.isTerminal) {
          // Terminal state decides done vs error
          this.webContents.send('projection-event', {
            type: 'fwUpdate',
            stage: msg.ok ? 'upload:done' : 'upload:error',
            message: msg.statusText || (msg.ok ? 'Update finished' : 'Update failed'),
            status: msg.status,
            isOta: msg.isOta
          })

          // Ensure the next SoftwareVersion/BoxInfo triggers a fresh emit.
          this.lastDongleInfoEmitKey = ''

          // Force a fresh dongleInfo emit AFTER the dongle reports new SoftwareVersion/BoxInfo.
          try {
            this.driver.send(new SendCommand('frame'))
          } catch {
            // ignore
          }
        }
      } else if (msg instanceof VideoData) {
        const isNavi = msg.header.type === MessageType.NaviVideoData
        // navi video stream (0x2c)
        if (isNavi) {
          if (!this.mapsRequested) return

          const w = msg.width
          const h = msg.height

          if (w > 0 && h > 0 && (w !== this.lastNaviVideoWidth || h !== this.lastNaviVideoHeight)) {
            this.lastNaviVideoWidth = w
            this.lastNaviVideoHeight = h
            this.webContents.send('maps-video-resolution', { width: w, height: h })
          }

          this.sendChunked('maps-video-chunk', msg.data?.buffer as ArrayBuffer, 512 * 1024)
          return
        }

        // main video stream (0x06)
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

          this.webContents.send('projection-event', {
            type: 'resolution',
            payload: { width: w, height: h }
          })
        }

        this.sendChunked('projection-video-chunk', msg.data?.buffer as ArrayBuffer, 512 * 1024)
      } else if (msg instanceof AudioData) {
        this.audio.handleAudioData(msg)

        if (msg.command != null) {
          if (this.lastPluggedPhoneType === PhoneType.AndroidAuto) {
            if (msg.command === 10) {
              this.aaPlaybackInferred = 1
              this.patchAaMediaPlayStatus(1)
            }
            if (msg.command === 11 || msg.command === 2) {
              this.aaPlaybackInferred = 2
              this.patchAaMediaPlayStatus(2)
            }
          }

          this.webContents.send('projection-event', {
            type: 'audio',
            payload: {
              command: msg.command,
              audioType: msg.audioType,
              decodeType: msg.decodeType,
              volume: msg.volume
            }
          })
        }

        const fmt = decodeTypeMap[msg.decodeType]
        if (!fmt) return

        const key = `${msg.decodeType}|${msg.audioType}|${fmt.frequency}|${fmt.channel}|${fmt.bitDepth}`
        if (key === this.lastAudioMetaEmitKey) return
        this.lastAudioMetaEmitKey = key

        this.webContents.send('projection-event', {
          type: 'audioInfo',
          payload: {
            codec: fmt.format ?? msg.decodeType ?? 'unknown',
            sampleRate: fmt.frequency,
            channels: fmt.channel,
            bitDepth: fmt.bitDepth
          }
        })
      } else if (msg instanceof MetaData) {
        const inner = msg.inner

        // Media metadata (innerType 1/3/100)
        if (inner.kind === 'media') {
          const mediaMsg = inner.message
          if (!mediaMsg.payload) return

          this.webContents.send('projection-event', { type: 'media', payload: mediaMsg })

          const file = path.join(app.getPath('userData'), 'mediaData.json')
          const existing = readMediaFile(file)
          const existingPayload = existing.payload
          const newPayload: PersistedMediaPayload = { type: mediaMsg.payload.type }

          if (mediaMsg.payload.type === MediaType.Data && mediaMsg.payload.media) {
            const mergedMedia = { ...existingPayload.media, ...mediaMsg.payload.media }

            if (
              this.lastPluggedPhoneType === PhoneType.AndroidAuto &&
              mergedMedia.MediaPlayStatus === undefined
            ) {
              mergedMedia.MediaPlayStatus = this.aaPlaybackInferred
            }

            newPayload.media = mergedMedia
            if (existingPayload.base64Image) newPayload.base64Image = existingPayload.base64Image
          } else if ('base64Image' in mediaMsg.payload && mediaMsg.payload.base64Image) {
            newPayload.base64Image = mediaMsg.payload.base64Image
            if (existingPayload.media) newPayload.media = existingPayload.media
          } else {
            newPayload.media = existingPayload.media
            newPayload.base64Image = existingPayload.base64Image
          }

          const out = { timestamp: new Date().toISOString(), payload: newPayload }
          fs.writeFileSync(file, JSON.stringify(out, null, 2), 'utf8')
          return
        }

        // Navigation metadata (innerType 200/201)
        if (inner.kind === 'navigation') {
          if (!this.started) return
          const navMsg = inner.message

          this.webContents.send('projection-event', { type: 'navigation', payload: navMsg })

          const file = path.join(app.getPath('userData'), 'navigationData.json')
          const existing = readNavigationFile(file)

          const locale: NavLocale =
            this.config.language === 'de'
              ? 'de'
              : this.config.language === 'ua' ||
                  this.config.language === 'uk' ||
                  this.config.language === 'uk-UA'
                ? 'ua'
                : 'en'

          const normalized = normalizeNavigationPayload(existing.payload, navMsg)
          const translated = translateNavigation(normalized.navi, locale)

          const nextPayload: PersistedNavigationPayload = {
            ...normalized,
            display: {
              locale,
              appName: translated.SourceName,
              destinationName: translated.DestinationName,
              roadName: translated.CurrentRoadName,
              maneuverText: translated.ManeuverTypeText,
              timeToDestinationText: translated.TimeRemainingToDestinationText,
              distanceToDestinationText: translated.DistanceRemainingDisplayStringText,
              remainDistanceText: translated.RemainDistanceText
            }
          }

          const out = { timestamp: new Date().toISOString(), payload: nextPayload }
          fs.writeFileSync(file, JSON.stringify(out, null, 2), 'utf8')

          return
        }
        // Unknown meta
      } else if (msg instanceof Command) {
        this.webContents.send('projection-event', { type: 'command', message: msg })
        if (typeof msg.value === 'number' && msg.value === 508 && this.mapsRequested) {
          try {
            this.driver.send(new SendCommand('requestNaviScreenFocus'))
          } catch {
            // ignore
          }
        }
      }
    })

    this.driver.on('failure', () => {
      this.webContents?.send('projection-event', { type: 'failure' })
    })

    // TODO move IPC registration to dedicated IPC modules instead of service constructor
    registerIpcHandle('projection-start', async () => this.start())
    registerIpcHandle('projection-stop', async () => this.stop())
    registerIpcHandle('projection-sendframe', async () =>
      this.driver.send(new SendCommand('frame'))
    )

    registerIpcHandle('projection-bt-pairedlist-set', async (_evt, listText: string) => {
      if (!this.started) return { ok: false }
      const ok = await this.driver.sendBluetoothPairedList(String(listText ?? ''))
      return { ok }
    })

    registerIpcHandle('projection-upload-icons', async () => {
      if (!this.started || !this.webUsbDevice) {
        throw new Error('[ProjectionService] Projection is not started or dongle not connected')
      }
      this.uploadIcons()
    })

    registerIpcHandle('projection-upload-livi-scripts', async (): Promise<DevToolsUploadResult> => {
      if (!this.started || !this.webUsbDevice) {
        throw new Error('[ProjectionService] Projection is not started or dongle not connected')
      }

      const startedAtMs = Date.now()
      const startedAt = new Date(startedAtMs).toISOString()
      console.info('[ProjectionService] Dev tools upload started')

      const cgiOk = await this.driver.send(new SendServerCgiScript())
      const webOk = await this.driver.send(new SendLiviWeb())
      const urls = this.getDevToolsUrlCandidates()

      const finishedAtMs = Date.now()
      const finishedAt = new Date(finishedAtMs).toISOString()
      const result: DevToolsUploadResult = {
        ok: Boolean(cgiOk && webOk),
        cgiOk: Boolean(cgiOk),
        webOk: Boolean(webOk),
        urls,
        startedAt,
        finishedAt,
        durationMs: finishedAtMs - startedAtMs
      }

      console.info('[ProjectionService] Dev tools upload finished', result)
      return result
    })

    registerIpcOn('projection-touch', (_evt, data: { x: number; y: number; action: number }) => {
      try {
        this.driver.send(new SendTouch(data.x, data.y, data.action))
      } catch {
        // ignore
      }
    })

    registerIpcHandle('maps:request', async (_evt, enabled: boolean) => {
      this.mapsRequested = Boolean(enabled)

      if (!this.mapsRequested) {
        this.lastNaviVideoWidth = undefined
        this.lastNaviVideoHeight = undefined
        return { ok: true, enabled: false }
      }

      try {
        this.driver.send(new SendCommand('requestNaviScreenFocus'))
      } catch {
        // ignore
      }

      return { ok: true, enabled: true }
    })

    type MultiTouchPoint = { id: number; x: number; y: number; action: number }
    const to01 = (v: number): number => {
      const n = Number.isFinite(v) ? v : 0
      return n < 0 ? 0 : n > 1 ? 1 : n
    }
    const ONE_BASED_IDS = false

    registerIpcOn('projection-multi-touch', (_evt, points: MultiTouchPoint[]) => {
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

    registerIpcOn('projection-raw-message', (_evt, payload: { type: number; data: number[] }) => {
      try {
        if (!this.started) return

        const msg = new SendRawMessage(payload.type, new Uint8Array(payload.data ?? []))
        this.driver.send(msg)
      } catch (e) {
        console.error('[ProjectionService] projection-raw-message failed', e)
      }
    })

    registerIpcOn(
      'projection-command',
      (_evt, command: ConstructorParameters<typeof SendCommand>[0]) => {
        this.driver.send(new SendCommand(command))
      }
    )

    registerIpcHandle('projection-media-read', async () => {
      try {
        const file = path.join(app.getPath('userData'), 'mediaData.json')

        if (!fs.existsSync(file)) {
          console.log('[projection-media-read] Error: ENOENT: no such file or directory')
          return DEFAULT_MEDIA_DATA_RESPONSE
        }

        return readMediaFile(file)
      } catch (error) {
        console.log('[projection-media-read]', error)
        return DEFAULT_MEDIA_DATA_RESPONSE
      }
    })

    registerIpcHandle('projection-navigation-read', async () => {
      try {
        if (!this.started) {
          return DEFAULT_NAVIGATION_DATA_RESPONSE
        }

        const file = path.join(app.getPath('userData'), 'navigationData.json')

        if (!fs.existsSync(file)) {
          console.log('[projection-navigation-read] Error: ENOENT: no such file or directory')
          return DEFAULT_NAVIGATION_DATA_RESPONSE
        }

        return readNavigationFile(file)
      } catch (error) {
        console.log('[projection-navigation-read]', error)
        return DEFAULT_NAVIGATION_DATA_RESPONSE
      }
    })

    // ============================
    // Dongle firmware updater IPC
    // ============================
    registerIpcHandle(
      'dongle-fw',
      async (_evt, req: DongleFirmwareRequest): Promise<DongleFwCheckResponse> => {
        await this.reloadConfigFromDisk()

        const asError = (message: string): DongleFwCheckResponse => ({
          ok: false,
          hasUpdate: false,
          size: 0,
          error: message,
          raw: { err: -1, msg: message }
        })

        const toRendererShape = (r: FirmwareCheckResult): DongleFwCheckResponse => {
          if (!r.ok) return asError(r.error || 'Unknown error')

          const rawObj: Record<string, unknown> = isRecord(r.raw) ? r.raw : {}

          const rawErr = pickNumber(rawObj, 'err') ?? 0
          const rawToken = pickString(rawObj, 'token')
          const rawVer = pickString(rawObj, 'ver')
          const rawSize = pickStringOrNumber(rawObj, 'size')
          const rawId = pickString(rawObj, 'id')
          const rawNotes = pickString(rawObj, 'notes')
          const rawMsg = pickString(rawObj, 'msg')
          const rawError = pickString(rawObj, 'error')

          return {
            ok: true,
            hasUpdate: Boolean(r.hasUpdate),
            size: typeof r.size === 'number' ? r.size : 0,
            token: r.token,
            request: isRecord(r.request) ? r.request : undefined,
            raw: {
              err: rawErr,
              token: r.token ?? rawToken,
              ver: r.latestVer ?? rawVer,
              size: (typeof r.size === 'number' ? r.size : rawSize) ?? 0,
              id: r.id ?? rawId,
              notes: r.notes ?? rawNotes,
              msg: rawMsg,
              error: rawError
            }
          }
        }
        const action = req?.action

        if (action === 'check') {
          this.webContents?.send('projection-event', { type: 'fwUpdate', stage: 'check:start' })

          const result = await this.firmware.checkForUpdate({
            appVer: this.getApkVer(),
            dongleFwVersion: this.dongleFwVersion ?? null,
            boxInfo: this.boxInfo
          })

          const shaped = toRendererShape(result)

          this.webContents?.send('projection-event', {
            type: 'fwUpdate',
            stage: 'check:done',
            result: shaped
          })

          return shaped
        }

        if (action === 'download') {
          try {
            this.webContents?.send('projection-event', {
              type: 'fwUpdate',
              stage: 'download:start'
            })

            const check = await this.firmware.checkForUpdate({
              appVer: this.getApkVer(),
              dongleFwVersion: this.dongleFwVersion ?? null,
              boxInfo: this.boxInfo
            })

            const shapedCheck = toRendererShape(check)

            if (!check.ok) {
              const msg = check.error || 'checkForUpdate failed'
              this.webContents?.send('projection-event', {
                type: 'fwUpdate',
                stage: 'download:error',
                message: msg
              })
              return asError(msg)
            }

            if (!check.hasUpdate) {
              this.webContents?.send('projection-event', {
                type: 'fwUpdate',
                stage: 'download:done',
                path: null,
                bytes: 0
              })
              return shapedCheck
            }

            const dl = await this.firmware.downloadFirmwareToHost(check, {
              overwrite: true,
              onProgress: (p) => {
                this.webContents?.send('projection-event', {
                  type: 'fwUpdate',
                  stage: 'download:progress',
                  received: p.received,
                  total: p.total,
                  percent: p.percent
                })
              }
            })

            if (!dl.ok) {
              const msg = dl.error || 'download failed'
              this.webContents?.send('projection-event', {
                type: 'fwUpdate',
                stage: 'download:error',
                message: msg
              })
              return asError(msg)
            }

            this.webContents?.send('projection-event', {
              type: 'fwUpdate',
              stage: 'download:done',
              path: dl.path,
              bytes: dl.bytes
            })

            return shapedCheck
          } catch (e) {
            const msg = e instanceof Error ? e.message : String(e)
            this.webContents?.send('projection-event', {
              type: 'fwUpdate',
              stage: 'download:error',
              message: msg
            })
            return asError(msg)
          }
        }

        if (action === 'upload') {
          try {
            if (!this.started) return asError('Projection not started / dongle not connected')

            this.webContents?.send('projection-event', { type: 'fwUpdate', stage: 'upload:start' })

            const st = await this.firmware.getLocalFirmwareStatus({
              appVer: this.getApkVer(),
              dongleFwVersion: this.dongleFwVersion ?? null,
              boxInfo: this.boxInfo
            })

            if (!st || st.ok !== true) {
              const msg = String(st?.error || 'Local firmware status failed')
              this.webContents?.send('projection-event', {
                type: 'fwUpdate',
                stage: 'upload:error',
                message: msg
              })
              return asError(msg)
            }

            if (!st.ready) {
              const msg = String(st.reason || 'No firmware ready to upload')
              this.webContents?.send('projection-event', {
                type: 'fwUpdate',
                stage: 'upload:error',
                message: msg
              })
              return asError(msg)
            }

            const fwBuf = await fs.promises.readFile(st.path)
            const remotePath = `/tmp/${path.basename(st.path)}`

            const ok = await this.driver.send(new SendFile(fwBuf, remotePath))
            if (!ok) {
              const msg = 'Dongle upload failed (SendFile returned false)'
              this.webContents?.send('projection-event', {
                type: 'fwUpdate',
                stage: 'upload:error',
                message: msg
              })
              return asError(msg)
            }

            this.webContents?.send('projection-event', {
              type: 'fwUpdate',
              stage: 'upload:file-sent',
              path: remotePath,
              bytes: fwBuf.length
            })
            return {
              ok: true,
              hasUpdate: true,
              size: fwBuf.length,
              token: undefined,
              request: { uploadedTo: remotePath, local: st },
              raw: { err: 0, msg: 'upload:file-sent', size: fwBuf.length }
            }
          } catch (e) {
            const msg = e instanceof Error ? e.message : String(e)
            this.webContents?.send('projection-event', {
              type: 'fwUpdate',
              stage: 'upload:error',
              message: msg
            })
            return asError(msg)
          }
        }

        if (action === 'status') {
          const st = await this.firmware.getLocalFirmwareStatus({
            appVer: this.getApkVer(),
            dongleFwVersion: this.dongleFwVersion ?? null,
            boxInfo: this.boxInfo
          })

          if (!st) {
            return asError('Local firmware status failed')
          }

          if (st.ok !== true) {
            return asError(typeof st.error === 'string' ? st.error : 'Local firmware status failed')
          }

          if (!st.ready) {
            return {
              ok: true,
              hasUpdate: false,
              size: 0,
              token: undefined,
              request: { local: st },
              raw: {
                err: 0,
                msg: 'local:not-ready'
              }
            }
          }

          const latestVer = typeof st.latestVer === 'string' ? st.latestVer : undefined
          const bytes = st.bytes

          return {
            ok: true,
            hasUpdate: Boolean(latestVer),
            size: bytes,
            token: undefined,
            request: { local: st },
            raw: {
              err: 0,
              ver: latestVer,
              size: bytes,
              msg: 'local:ready'
            }
          }
        }

        return asError(`Unknown action: ${String(action)}`)
      }
    )

    registerIpcOn(
      'projection-set-volume',
      (_evt, payload: { stream: LogicalStreamKey; volume: number }) => {
        const { stream, volume } = payload || {}
        this.audio.setStreamVolume(stream, volume)
      }
    )

    // visualizer / FFT toggle from renderer
    registerIpcOn('projection-set-visualizer-enabled', (_evt, enabled: boolean) => {
      this.audio.setVisualizerEnabled(Boolean(enabled))
    })
    this.subscribeConfigEvents()
  }

  private async reloadConfigFromDisk(): Promise<void> {
    try {
      const configPath = path.join(app.getPath('userData'), 'config.json')
      if (!fs.existsSync(configPath)) return
      const userConfig = JSON.parse(fs.readFileSync(configPath, 'utf8')) as ExtraConfig
      this.config = { ...this.config, ...userConfig }
    } catch {
      // ignore
    }
  }

  private getApkVer(): string {
    return this.config.apkVer
  }

  private getDevToolsUrlCandidates(): string[] {
    // Keep this list intentionally strict to avoid opening unrelated LAN gateways
    // such as 192.168.0.1/192.168.1.1 from the user's home network.
    // TODO Move default IP list to the constants
    const ipSet = new Set<string>(['192.168.50.1', '192.168.43.1', '192.168.3.1'])

    const paths = ['/', '/index.html', '/cgi-bin/server.cgi?action=ls&path=/']
    const out: string[] = []
    for (const host of ipSet) {
      for (const p of paths) out.push(`http://${host}${p}`)
    }
    return out
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
          '[ProjectionService] failed to reload config.json before icon upload, using in-memory config',
          err
        )
      }

      const b120 = cfg.dongleIcon120 ? cfg.dongleIcon120.trim() : ''
      const b180 = cfg.dongleIcon180 ? cfg.dongleIcon180.trim() : ''
      const b256 = cfg.dongleIcon256 ? cfg.dongleIcon256.trim() : ''

      if (!b120 || !b180 || !b256) {
        console.error('[ProjectionService] Icon fields missing in config.json — upload cancelled')
        return
      }

      const buf120 = Buffer.from(b120, 'base64')
      const buf180 = Buffer.from(b180, 'base64')
      const buf256 = Buffer.from(b256, 'base64')

      this.driver.send(new SendFile(buf120, FileAddress.ICON_120))
      this.driver.send(new SendFile(buf180, FileAddress.ICON_180))
      this.driver.send(new SendFile(buf256, FileAddress.ICON_256))

      console.debug('[ProjectionService] uploaded icons from fresh config.json')
    } catch (err) {
      console.error('[ProjectionService] failed to upload icons', err)
    }
  }

  public attachRenderer(webContents: WebContents) {
    this.webContents = webContents
  }

  public applyConfigPatch(patch: Partial<ExtraConfig>): void {
    this.config = { ...this.config, ...patch }
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

    this.webContents.send('projection-event', {
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
        await this.reloadConfigFromDisk()

        const ext = this.config as VolumeConfig
        this.audio.setInitialVolumes({
          music: typeof ext.audioVolume === 'number' ? ext.audioVolume : undefined,
          nav: typeof ext.navVolume === 'number' ? ext.navVolume : undefined,
          siri: typeof ext.siriVolume === 'number' ? ext.siriVolume : undefined,
          call: typeof ext.callVolume === 'number' ? ext.callVolume : undefined
        })

        this.audio.resetForSessionStart()

        this.dongleFwVersion = undefined
        this.boxInfo = undefined
        this.lastDongleInfoEmitKey = ''
        this.lastVideoWidth = undefined
        this.lastVideoHeight = undefined
        this.lastPluggedPhoneType = undefined
        this.aaPlaybackInferred = 1

        this.resetMediaSnapshot('session-start')
        this.resetNavigationSnapshot('session-start')

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
      console.warn('[ProjectionService] SendDisconnectPhone failed', e)
    }

    try {
      ok = (await this.driver.send(new SendCloseDongle())) || ok
    } catch (e) {
      console.warn('[ProjectionService] SendCloseDongle failed', e)
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
        console.warn('[ProjectionService] webUsbDevice.reset() failed (ignored)', e)
      }

      try {
        await this.driver.close()
      } catch (e) {
        console.warn('[ProjectionService] driver.close() failed (ignored)', e)
      }

      this.webUsbDevice = null
      this.audio.resetForSessionStop()

      this.started = false
      this.resetMediaSnapshot('session-stop')
      this.resetNavigationSnapshot('session-stop')

      this.dongleFwVersion = undefined
      this.boxInfo = undefined
      this.lastDongleInfoEmitKey = ''
      this.lastVideoWidth = undefined
      this.lastVideoHeight = undefined
      this.lastPluggedPhoneType = undefined
      this.aaPlaybackInferred = 2
    })().finally(() => {
      this.stopping = false
      this.isStopping = false
      this.stopPromise = null
    })

    return this.stopPromise
  }

  private patchAaMediaPlayStatus(status: 1 | 2): void {
    try {
      const file = path.join(app.getPath('userData'), 'mediaData.json')
      const existing = readMediaFile(file)

      const nextPayload: PersistedMediaPayload = {
        ...existing.payload,
        type: MediaType.Data,
        media: {
          ...existing.payload.media,
          MediaPlayStatus: status
        }
      }

      const out = {
        timestamp: new Date().toISOString(),
        payload: nextPayload
      }

      fs.writeFileSync(file, JSON.stringify(out, null, 2), 'utf8')

      this.webContents?.send('projection-event', {
        type: 'media',
        payload: {
          mediaType: MediaType.Data,
          payload: {
            type: MediaType.Data,
            media: {
              MediaPlayStatus: status
            }
          }
        }
      })
    } catch (e) {
      console.warn('[ProjectionService] patchAaMediaPlayStatus failed (ignored)', e)
    }
  }

  private resetMediaSnapshot(reason: string): void {
    try {
      const file = path.join(app.getPath('userData'), 'mediaData.json')

      const out = {
        timestamp: new Date().toISOString(),
        payload: DEFAULT_MEDIA_DATA_RESPONSE.payload
      }

      fs.writeFileSync(file, JSON.stringify(out, null, 2), 'utf8')
    } catch (e) {
      console.warn('[ProjectionService] resetMediaSnapshot failed (ignored)', reason, e)
    }

    this.webContents?.send('projection-event', { type: 'media-reset', reason })
  }

  private resetNavigationSnapshot(reason: string): void {
    try {
      const file = path.join(app.getPath('userData'), 'navigationData.json')

      const out = {
        timestamp: new Date().toISOString(),
        payload: DEFAULT_NAVIGATION_DATA_RESPONSE.payload
      }

      fs.writeFileSync(file, JSON.stringify(out, null, 2), 'utf8')
    } catch (e) {
      console.warn('[ProjectionService] resetNavigationSnapshot failed (ignored)', reason, e)
    }

    this.webContents?.send('projection-event', { type: 'navigation-reset', reason })
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

function asObject(input: unknown): Record<string, unknown> | null {
  if (!input) return null

  if (typeof input === 'object' && input !== null) return input as Record<string, unknown>

  if (typeof input === 'string') {
    const s = input.trim()
    if (!s) return null
    try {
      const parsed = JSON.parse(s)
      if (parsed && typeof parsed === 'object') return parsed as Record<string, unknown>
    } catch {
      // ignore
    }
  }

  return null
}

function isMeaningful(v: unknown): boolean {
  if (v == null) return false
  if (typeof v === 'string') return v.trim().length > 0
  return true
}

function mergePreferExisting(prev: unknown, next: unknown): unknown {
  const p = asObject(prev)
  const n = asObject(next)

  if (!p && !n) return next ?? prev
  if (!p && n) return next
  if (p && !n) return prev

  // both objects
  const out: Record<string, unknown> = { ...p }

  for (const [k, v] of Object.entries(n!)) {
    if (isMeaningful(v)) {
      out[k] = v
    } else {
      // keep existing if present
      if (!(k in out)) out[k] = v
    }
  }

  return out
}
