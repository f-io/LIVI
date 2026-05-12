import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'
import { broadcastToSecondaryRenderers } from '@main/window/broadcast'
import { getSecondaryWindow } from '@main/window/secondaryWindows'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { DevListEntry, DongleFirmwareAction, DongleFwApiRaw, ExtraConfig } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import type { NavLocale } from '@shared/utils'
import { isClusterDisplayed, translateNavigation } from '@shared/utils'
import { app, WebContents } from 'electron'
import fs from 'fs'
import path from 'path'
import { type Device, usb, WebUSBDevice } from 'usb'
import { AaBtSockClient } from '../driver/aa/AaBtSockClient'
import { AaDriver } from '../driver/aa/aaDriver'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import {
  AudioData,
  BluetoothPairedList,
  BoxInfo,
  BoxUpdateProgress,
  BoxUpdateState,
  Command,
  DEFAULT_CONFIG,
  DongleDriver,
  decodeTypeMap,
  FileAddress,
  GnssData,
  MediaType,
  type Message,
  MessageType,
  MetaData,
  PhoneType,
  Plugged,
  SendAudio,
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone,
  SendFile,
  SendForgetBluetoothAddr,
  SendLiviWeb,
  SendMultiTouch,
  SendRawMessage,
  SendServerCgiScript,
  SendTouch,
  SoftwareVersion,
  Unplugged,
  VideoData
} from '../messages'
import {
  APP_START_TS,
  DEFAULT_MEDIA_DATA_RESPONSE,
  DEFAULT_NAVIGATION_DATA_RESPONSE,
  DEVTOOLS_IP_CANDIDATES
} from './constants'
import { FirmwareCheckResult, FirmwareUpdateService } from './FirmwareUpdateService'
import { LogicalStreamKey, ProjectionAudio } from './ProjectionAudio'
import {
  type PendingStartupConnectTarget,
  PersistedMediaPayload,
  PersistedNavigationPayload
} from './types'
import { asDomUSBDevice } from './utils/asDomUSBDevice'
import { normalizeNavigationPayload } from './utils/normalizeNavigation'
import { readMediaFile } from './utils/readMediaFile'
import { readNavigationFile } from './utils/readNavigationFile'

let dongleConnected = false
let wiredPhoneConnected = false
let wiredPhoneDevice: Device | null = null
let expectingPhoneReenumUntil = 0

type VolumeConfig = {
  audioVolume?: number
  navVolume?: number
  voiceAssistantVolume?: number
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

/** appearanceMode → initial NIGHT_DATA bit for AA. 'auto' = no override (undefined). */
function deriveInitialNightMode(mode: string | undefined): boolean | undefined {
  if (mode === 'night') return true
  if (mode === 'day') return false
  return undefined
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
  private readonly dongleDriver = new DongleDriver()
  private aaDriver: AaDriver | null = null
  private get driver(): IPhoneDriver {
    return this.aaDriver ?? this.dongleDriver
  }
  public getAaDriver(): AaDriver | null {
    return this.aaDriver
  }
  public getDongleDriver(): DongleDriver {
    return this.dongleDriver
  }
  private hevcSupported = false
  private vp9Supported = false
  private av1Supported = false
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
  private readonly aaBtSock = new AaBtSockClient()
  private aaBtSubscription: { close: () => void } | null = null

  private readonly onAaConnected = (): void => {
    this.refreshAaBtPairedList().catch(() => {})
  }
  private readonly onAaDisconnected = (): void => {
    this.refreshAaBtPairedList().catch(() => {})
  }

  // Hydration
  private readonly pluggedHooks: Array<(phoneType: PhoneType) => void> = []
  public addPluggedHook(fn: (phoneType: PhoneType) => void): () => void {
    this.pluggedHooks.push(fn)
    return (): void => {
      const i = this.pluggedHooks.indexOf(fn)
      if (i >= 0) this.pluggedHooks.splice(i, 1)
    }
  }

  private lastClusterVideoWidth?: number
  private lastClusterVideoHeight?: number
  private clusterRequested = false
  private lastClusterCodec: 'h264' | 'h265' | 'vp9' | 'av1' | null = null

  // Per-channel buffers for video chunks that arrive from the phone before
  // the renderer is attached. Keeping main and cluster separate matters:
  // at 60fps main video would otherwise push the cluster's initial SPS/IDR
  // out of a shared FIFO before the renderer is up.
  private earlyVideoQueues: Map<string, Array<Record<string, unknown>>> = new Map()
  private static readonly EARLY_QUEUE_MAX_PER_CHANNEL = 256
  private lastPluggedPhoneType?: PhoneType
  private aaPlaybackInferred: 1 | 2 = 1
  private pendingStartupConnectTarget: PendingStartupConnectTarget | null = null

  private audio: ProjectionAudio

  private readonly onConfigChanged = (next: ExtraConfig) => {
    if (this.shuttingDown) return
    const prev = this.config
    this.config = { ...this.config, ...next }

    const hwToggled =
      typeof next.hwAcceleration === 'boolean' && next.hwAcceleration !== prev?.hwAcceleration
    const prevClusterActive = isClusterDisplayed(prev)
    const nextClusterActive = isClusterDisplayed(this.config)
    const clusterToggled = prevClusterActive !== nextClusterActive

    if (hwToggled) {
      this.recomputeCodecCapabilities()
    }

    if (hwToggled || clusterToggled) {
      console.log(
        `[ProjectionService] config change requires AA restart (hw=${hwToggled}, cluster=${clusterToggled})`
      )
      this.aaDriver?.restartStack().catch((e) => {
        console.warn('[ProjectionService] AA stack restart on config toggle failed', e)
      })
    }

    if (clusterToggled && !nextClusterActive) {
      // Drop any in-flight cluster request so we stop forwarding cluster chunks
      // to the renderer until the next explicit request.
      this.clusterRequested = false
      this.lastClusterCodec = null
      this.lastClusterVideoWidth = undefined
      this.lastClusterVideoHeight = undefined
    }

    // Seed AA's initial NIGHT_MODE
    if (next.appearanceMode !== prev?.appearanceMode) {
      this.aaDriver?.setInitialNightMode(deriveInitialNightMode(next.appearanceMode))
    }

    const prefChanged =
      (prev as { connectionPreference?: unknown })?.connectionPreference !==
      (next as { connectionPreference?: unknown })?.connectionPreference
    if (prefChanged) {
      this.nativeProbeDeferred = false
      this.nativeProbeDeadline = 0
      this.emitTransportState()
    }
  }

  private lastCodecCaps: Record<string, { hw?: unknown; sw?: unknown } | undefined> | null = null

  // Single emit point for `projection-event`
  private emitProjectionEvent(payload: unknown): void {
    this.webContents?.send('projection-event', payload)
    broadcastToSecondaryRenderers('projection-event', payload)
  }

  private applyCodecCapabilities(payload: unknown): void {
    if (!payload || typeof payload !== 'object') return
    const caps = payload as Record<string, { hw?: unknown; sw?: unknown } | undefined>
    this.lastCodecCaps = caps
    this.recomputeCodecCapabilities()
  }

  private recomputeCodecCapabilities(): void {
    const caps = this.lastCodecCaps
    if (!caps) return
    const useHw = Boolean(this.config.hwAcceleration)
    const isSupported = (c: { hw?: unknown; sw?: unknown } | undefined): boolean => {
      if (!c) return false
      return useHw ? Boolean(c.hw) || Boolean(c.sw) : Boolean(c.sw)
    }

    const hevc = isSupported(caps.h265)
    const vp9 = isSupported(caps.vp9)
    const av1 = isSupported(caps.av1)

    if (this.hevcSupported !== hevc) {
      this.hevcSupported = hevc
      console.log(
        `[ProjectionService] hevc support: ${hevc} (useHw=${useHw}, hw=${Boolean(caps.h265?.hw)}, sw=${Boolean(caps.h265?.sw)})`
      )
      this.aaDriver?.setHevcSupported(hevc)
    }
    if (this.vp9Supported !== vp9) {
      this.vp9Supported = vp9
      console.log(
        `[ProjectionService] vp9 support: ${vp9} (useHw=${useHw}, hw=${Boolean(caps.vp9?.hw)}, sw=${Boolean(caps.vp9?.sw)})`
      )
      this.aaDriver?.setVp9Supported(vp9)
    }
    if (this.av1Supported !== av1) {
      this.av1Supported = av1
      console.log(
        `[ProjectionService] av1 support: ${av1} (useHw=${useHw}, hw=${Boolean(caps.av1?.hw)}, sw=${Boolean(caps.av1?.sw)})`
      )
      this.aaDriver?.setAv1Supported(av1)
    }
  }

  /** Read by AaDriver right before it starts the AAStack. */
  public getHevcSupported(): boolean {
    return this.hevcSupported
  }

  private readonly onDriverMessage = (msg: Message): void => {
    // Always keep updater-relevant state, even if renderer is not attached yet.
    if (msg instanceof SoftwareVersion) {
      this.dongleFwVersion = msg.version
      this.emitDongleInfoIfChanged()
      return
    }

    if (msg instanceof BoxInfo) {
      const settings = msg.settings as { DevList?: Array<Record<string, unknown>> }
      if (Array.isArray(settings.DevList)) {
        settings.DevList = settings.DevList.map((entry) => ({
          ...entry,
          source: 'dongle' as const
        }))
      }
      this.boxInfo = mergePreferExisting(this.boxInfo, msg.settings)
      this.emitDongleInfoIfChanged()
      return
    }

    if (msg instanceof GnssData) {
      this.emitProjectionEvent({
        type: 'gnss',
        payload: {
          text: msg.text
        }
      })
      return
    }

    if (!this.webContents) return

    if (msg instanceof BluetoothPairedList) {
      this.emitProjectionEvent({
        type: 'bluetoothPairedList',
        payload: msg.data
      })
      return
    }

    if (msg instanceof Plugged) {
      this.clearTimeouts()
      this.lastPluggedPhoneType = msg.phoneType
      this.aaPlaybackInferred = 1
      this.lastVideoWidth = undefined
      this.lastVideoHeight = undefined
      this.lastClusterVideoWidth = undefined
      this.lastClusterVideoHeight = undefined

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
      this.emitProjectionEvent({ type: 'plugged', phoneType: msg.phoneType })
      // Hydration
      for (const fn of this.pluggedHooks) {
        try {
          fn(msg.phoneType)
        } catch (e) {
          console.warn('[ProjectionService] plugged hook threw (ignored)', e)
        }
      }
      if (!this.started && !this.isStarting) {
        this.start().catch(() => {})
      }
    } else if (msg instanceof Unplugged) {
      this.clearTimeouts()
      this.lastPluggedPhoneType = undefined
      this.aaPlaybackInferred = 1

      if (isRecord(this.boxInfo)) {
        this.boxInfo = { ...this.boxInfo, btMacAddr: '' }
      }

      this.emitProjectionEvent({ type: 'unplugged' })
      this.emitProjectionEvent({
        type: 'dongleInfo',
        payload: {
          dongleFwVersion: this.dongleFwVersion,
          boxInfo: this.boxInfo
        }
      })
      this.resetNavigationSnapshot('unplugged')

      if (this.aaDriver) {
        try {
          this.audio.resetForSessionStop()
        } catch (e) {
          console.warn('[ProjectionService] audio reset on Unplugged threw (ignored)', e)
        }
        this.resetMediaSnapshot('aa-session-end')
        this.lastVideoWidth = undefined
        this.lastVideoHeight = undefined
        this.lastClusterVideoWidth = undefined
        this.lastClusterVideoHeight = undefined
      } else if (!this.shuttingDown && !this.stopping) {
        // Dongle mode: full stop is correct (no supervisor to keep alive).
        this.stop().catch(() => {})
      }
    } else if (msg instanceof BoxUpdateProgress) {
      // 0xb1 payload: int32 progress
      this.emitProjectionEvent({
        type: 'fwUpdate',
        stage: 'upload:progress',
        progress: msg.progress
      })
    } else if (msg instanceof BoxUpdateState) {
      // 0xbb payload: int32 status (start/success/fail, ota variants)
      this.emitProjectionEvent({
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
        this.emitProjectionEvent({
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
      const isCluster = msg.header.type === MessageType.ClusterVideoData
      // cluster video stream (0x2c)
      if (isCluster) {
        if (!isClusterDisplayed(this.config)) return

        const w = msg.width
        const h = msg.height

        const clusterTargets = this.getClusterTargetWebContents()

        if (
          w > 0 &&
          h > 0 &&
          (w !== this.lastClusterVideoWidth || h !== this.lastClusterVideoHeight)
        ) {
          this.lastClusterVideoWidth = w
          this.lastClusterVideoHeight = h
          for (const wc of clusterTargets) {
            if (!wc.isDestroyed()) wc.send('cluster-video-resolution', { width: w, height: h })
          }
        }

        this.sendChunked(
          'cluster-video-chunk',
          msg.data?.buffer as ArrayBuffer,
          512 * 1024,
          undefined,
          clusterTargets
        )
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

        this.emitProjectionEvent({
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

        this.emitProjectionEvent({
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

      this.emitProjectionEvent({
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

        this.emitProjectionEvent({ type: 'media', payload: mediaMsg })

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

        this.emitProjectionEvent({ type: 'navigation', payload: navMsg })

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
      this.emitProjectionEvent({ type: 'command', message: msg })
      if (typeof msg.value === 'number' && msg.value === 508 && this.clusterRequested) {
        try {
          this.driver.send(new SendCommand('requestClusterStreamFocus'))
        } catch {
          // ignore
        }
      }
    }
  }

  private readonly onDriverFailure = (): void => {
    const wc = this.webContents
    if (!wc || wc.isDestroyed?.()) return
    wc.send('projection-event', { type: 'failure' })
  }

  private readonly onDriverTargetedConnect = (): void => {
    this.pendingStartupConnectTarget = null
  }

  // 'video-codec' — phone announces which advertised codec it picked.
  private readonly onDriverVideoCodec = (codec: 'h264' | 'h265' | 'vp9' | 'av1'): void => {
    const wc = this.webContents
    if (!wc || wc.isDestroyed?.()) return
    wc.send('projection-event', { type: 'video-codec', payload: { codec } })
  }

  // Cluster channel codec selection
  private readonly onDriverClusterVideoCodec = (codec: 'h264' | 'h265' | 'vp9' | 'av1'): void => {
    this.lastClusterCodec = codec
    for (const wc of this.getClusterTargetWebContents()) {
      try {
        wc.send('projection-event', { type: 'cluster-video-codec', payload: { codec } })
      } catch {
        /* detached webContents */
      }
    }
  }

  private attachDriverListeners(d: IPhoneDriver): void {
    d.on('message', this.onDriverMessage)
    d.on('failure', this.onDriverFailure)
    d.on('targeted-connect-dispatched', this.onDriverTargetedConnect)
    d.on('video-codec', this.onDriverVideoCodec)
    d.on('cluster-video-codec', this.onDriverClusterVideoCodec)
  }

  private detachDriverListeners(d: IPhoneDriver): void {
    d.off('message', this.onDriverMessage)
    d.off('failure', this.onDriverFailure)
    d.off('targeted-connect-dispatched', this.onDriverTargetedConnect)
    d.off('video-codec', this.onDriverVideoCodec)
    d.off('cluster-video-codec', this.onDriverClusterVideoCodec)
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
        this.emitProjectionEvent(payload)
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

    this.attachDriverListeners(this.dongleDriver)

    // TODO move IPC registration to dedicated IPC modules instead of service constructor
    registerIpcHandle('projection-start', async () => this.start())
    registerIpcHandle('projection-stop', async () => {
      if (this.wantsAaDriver()) return
      return this.stop()
    })

    registerIpcHandle('projection-restart', async () => {
      try {
        await this.stop()
      } catch (e) {
        console.warn('[ProjectionService] projection-restart: stop threw (ignored)', e)
      }
      return this.start()
    })

    registerIpcHandle('transport:flip', async () => this.flipTransport())
    registerIpcHandle('transport:state', async () => this.getTransportState())
    registerIpcHandle('projection-sendframe', async () =>
      this.driver.send(new SendCommand('frame'))
    )

    registerIpcHandle('projection-codec-capabilities', async (_evt, caps: unknown) => {
      this.applyCodecCapabilities(caps)
    })

    registerIpcHandle('projection-bt-pairedlist-set', async (_evt, listText: string) => {
      if (!this.started) return { ok: false }

      if (this.driver instanceof DongleDriver) {
        const ok = await this.driver.sendBluetoothPairedList(String(listText ?? ''))
        return { ok }
      }
      return { ok: true }
    })

    registerIpcHandle('projection-bt-connect-device', async (_evt, mac: string) => {
      if (!this.started) return { ok: false }

      const btMac = String(mac ?? '').trim()
      if (!btMac) return { ok: false }

      // AaDriver path: drive BlueZ Device1.Connect through the python sock.
      if (this.aaDriver) {
        try {
          const resp = await this.aaBtSock.connect(btMac)
          if (resp.ok) this.refreshAaBtPairedList().catch(() => {})
          return resp
        } catch (e) {
          return { ok: false, error: (e as Error).message }
        }
      }

      const devList = Array.isArray((this.boxInfo as { DevList?: unknown[] } | undefined)?.DevList)
        ? ((this.boxInfo as { DevList?: Array<{ id?: string; type?: string }> }).DevList ?? [])
        : []

      const devEntry = devList.find((entry) => String(entry?.id ?? '').trim() === btMac)

      const targetPhoneWorkMode =
        devEntry?.type === 'AndroidAuto' ? PhoneWorkMode.Android : PhoneWorkMode.CarPlay

      this.pendingStartupConnectTarget = {
        btMac,
        phoneWorkMode: targetPhoneWorkMode
      }

      return { ok: true }
    })

    registerIpcHandle('projection-bt-forget-device', async (_evt, mac: string) => {
      if (!this.started) return { ok: false }

      const btMac = String(mac ?? '').trim()
      if (!btMac) return { ok: false }

      // AaDriver path: BlueZ Adapter1.RemoveDevice via the python sock.
      if (this.aaDriver) {
        try {
          const resp = await this.aaBtSock.remove(btMac)
          if (resp.ok) this.refreshAaBtPairedList().catch(() => {})
          return resp
        } catch (e) {
          return { ok: false, error: (e as Error).message }
        }
      }

      const ok = await this.driver.send(new SendForgetBluetoothAddr(btMac))
      return { ok: Boolean(ok) }
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

    registerIpcHandle('cluster:request', async (_evt, enabled: boolean) => {
      const allowed = Boolean(enabled) && isClusterDisplayed(this.config)
      this.clusterRequested = allowed

      if (!this.clusterRequested) {
        this.lastClusterVideoWidth = undefined
        this.lastClusterVideoHeight = undefined
        return { ok: true, enabled: false }
      }

      // Re-emit the cached cluster codec to every cluster target
      if (this.lastClusterCodec) {
        for (const wc of this.getClusterTargetWebContents()) {
          try {
            wc.send('projection-event', {
              type: 'cluster-video-codec',
              payload: { codec: this.lastClusterCodec }
            })
          } catch {
            /* detached webContents */
          }
        }
      }

      // Always re-send the focus indication. Phone now receives a single
      // PROJECTED indication on ch=19 — idempotent, safe to call on each
      // cluster route entry.
      try {
        this.driver.send(new SendCommand('requestClusterStreamFocus'))
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
        const file = path.join(app.getPath('userData'), 'navigationData.json')

        if (!fs.existsSync(file)) {
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
          this.emitProjectionEvent({ type: 'fwUpdate', stage: 'check:start' })

          const result = await this.firmware.checkForUpdate({
            appVer: this.getApkVer(),
            dongleFwVersion: this.dongleFwVersion ?? null,
            boxInfo: this.boxInfo
          })

          const shaped = toRendererShape(result)

          this.emitProjectionEvent({
            type: 'fwUpdate',
            stage: 'check:done',
            result: shaped
          })

          return shaped
        }

        if (action === 'download') {
          try {
            this.emitProjectionEvent({
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
              this.emitProjectionEvent({
                type: 'fwUpdate',
                stage: 'download:error',
                message: msg
              })
              return asError(msg)
            }

            if (!check.hasUpdate) {
              this.emitProjectionEvent({
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
                this.emitProjectionEvent({
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
              this.emitProjectionEvent({
                type: 'fwUpdate',
                stage: 'download:error',
                message: msg
              })
              return asError(msg)
            }

            this.emitProjectionEvent({
              type: 'fwUpdate',
              stage: 'download:done',
              path: dl.path,
              bytes: dl.bytes
            })

            return shapedCheck
          } catch (e) {
            const msg = e instanceof Error ? e.message : String(e)
            this.emitProjectionEvent({
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

            this.emitProjectionEvent({ type: 'fwUpdate', stage: 'upload:start' })

            const st = await this.firmware.getLocalFirmwareStatus({
              appVer: this.getApkVer(),
              dongleFwVersion: this.dongleFwVersion ?? null,
              boxInfo: this.boxInfo
            })

            if (!st || st.ok !== true) {
              const msg = String(st?.error || 'Local firmware status failed')
              this.emitProjectionEvent({
                type: 'fwUpdate',
                stage: 'upload:error',
                message: msg
              })
              return asError(msg)
            }

            if (!st.ready) {
              const msg = String(st.reason || 'No firmware ready to upload')
              this.emitProjectionEvent({
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
              this.emitProjectionEvent({
                type: 'fwUpdate',
                stage: 'upload:error',
                message: msg
              })
              return asError(msg)
            }

            this.emitProjectionEvent({
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
            this.emitProjectionEvent({
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
    const paths = ['/', '/index.html', '/cgi-bin/server.cgi?action=ls&path=/']
    return DEVTOOLS_IP_CANDIDATES.flatMap((host) => paths.map((p) => `http://${host}${p}`))
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

      const b120 = (cfg.dongleIcon120?.trim() || ICON_120_B64).trim()
      const b180 = (cfg.dongleIcon180?.trim() || ICON_180_B64).trim()
      const b256 = (cfg.dongleIcon256?.trim() || ICON_256_B64).trim()

      if (!b120 || !b180 || !b256) {
        console.error('[ProjectionService] Icon assets missing — upload cancelled')
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

    // Drain any video chunks that arrived from the phone before the renderer
    // window had finished loading. Per-channel so cluster IDR is preserved.
    if (this.earlyVideoQueues.size > 0) {
      const queues = this.earlyVideoQueues
      this.earlyVideoQueues = new Map()
      for (const [channel, queued] of queues) {
        console.log(
          `[ProjectionService] draining ${queued.length} early '${channel}' chunk(s) to attached renderer`
        )
        for (const envelope of queued) {
          try {
            if (typeof webContents.isDestroyed === 'function' && webContents.isDestroyed()) return
            webContents.send(channel, envelope)
          } catch {
            /* detached */
          }
        }
      }
    }
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

    this.emitProjectionEvent({
      type: 'dongleInfo',
      payload: {
        dongleFwVersion: this.dongleFwVersion,
        boxInfo: this.boxInfo
      }
    })
  }

  // Detach debounce. The bus produces transient detach events during:
  //   - libusb power-cycling the Carlinkit while idle (every ~10s on macOS,
  //     each "away" window 2-4s)
  //   - the AOAP reset inside stop() and during transport flips
  //   - Android phones cycling between OEM- and accessory-PID
  private static readonly DONGLE_DETACH_DEBOUNCE_MS = 4_000
  private static readonly PHONE_DETACH_DEBOUNCE_MS = 1_000
  private dongleDetachDebounce: NodeJS.Timeout | null = null
  private phoneDetachDebounce: NodeJS.Timeout | null = null

  public markDongleConnected(connected: boolean) {
    if (connected) {
      if (this.dongleDetachDebounce) {
        clearTimeout(this.dongleDetachDebounce)
        this.dongleDetachDebounce = null
      }
      if (dongleConnected) return
      dongleConnected = true
      this.emitTransportState()
      return
    }

    if (!dongleConnected) return
    if (this.dongleDetachDebounce) return
    // The dongle silently re-enumerates itself whenever it's not in use
    const usingDongle = this.started && !this.aaDriver
    const delay = usingDongle ? 0 : ProjectionService.DONGLE_DETACH_DEBOUNCE_MS
    this.dongleDetachDebounce = setTimeout(async () => {
      this.dongleDetachDebounce = null
      dongleConnected = false
      console.log('[ProjectionService] dongle marked disconnected')
      if (this.transportOverride === 'dongle') this.transportOverride = null

      if (this.started && !this.aaDriver) {
        try {
          await this.stop()
        } catch (e) {
          console.warn('[ProjectionService] stop after dongle unplug threw', e)
        }
      }

      this.emitTransportState()

      // Auto-switch to native if a candidate is still around
      if (this.hasNativeCandidate()) {
        this.autoStartIfNeeded().catch(console.error)
      }
    }, delay)
  }

  public markPhoneConnected(connected: boolean, device?: Device): void {
    if (connected) {
      if (this.phoneDetachDebounce) {
        clearTimeout(this.phoneDetachDebounce)
        this.phoneDetachDebounce = null
        wiredPhoneConnected = false
        wiredPhoneDevice = null
        console.log(
          '[ProjectionService] wired phone re-attach during detach debounce — committing detach inline'
        )
        if (this.transportOverride === 'aa') this.transportOverride = null
        if (this.started && this.aaDriver?.isWiredMode()) {
          void this.stop().catch((e) =>
            console.warn('[ProjectionService] stop on phone re-attach threw', e)
          )
        }
      }
      const wasConnected = wiredPhoneConnected
      wiredPhoneConnected = true
      wiredPhoneDevice = device ?? wiredPhoneDevice
      if (!wasConnected) {
        console.log('[ProjectionService] wired phone marked connected')
        this.autoStartIfNeeded().catch(console.error)
      }
      this.emitTransportState()
      return
    }

    if (!wiredPhoneConnected) return

    if (this.phoneDetachDebounce) return
    this.phoneDetachDebounce = setTimeout(() => {
      this.phoneDetachDebounce = null
      wiredPhoneConnected = false
      wiredPhoneDevice = null
      console.log('[ProjectionService] wired phone marked disconnected')
      if (this.transportOverride === 'aa') this.transportOverride = null
      if (this.started && this.aaDriver?.isWiredMode()) {
        void this.stop().catch((e) =>
          console.warn('[ProjectionService] stop after wired unplug threw', e)
        )
      }
      this.emitTransportState()
    }, ProjectionService.PHONE_DETACH_DEBOUNCE_MS)
  }

  public getWiredPhoneDevice(): Device | null {
    return wiredPhoneDevice
  }

  public isWiredPhoneConnected(): boolean {
    return wiredPhoneConnected
  }

  public expectPhoneReenumeration(durationMs: number): void {
    expectingPhoneReenumUntil = Date.now() + durationMs
  }

  public isExpectingPhoneReenumeration(): boolean {
    return Date.now() < expectingPhoneReenumUntil
  }

  private wantsAaDriver(): boolean {
    return this.pickPreferredTransport() === 'aa'
  }

  // Transport arbiter. Decides whether the next session should be brought up
  // on the dongle path or on the native path (wired or wireless).
  private transportOverride: 'dongle' | 'aa' | null = null

  private getConnectionPreference(): 'auto' | 'dongle' | 'native' {
    const raw = (this.config as { connectionPreference?: unknown }).connectionPreference
    return raw === 'dongle' || raw === 'native' ? raw : 'auto'
  }

  private hasNativeCandidate(): boolean {
    if (wiredPhoneConnected) return true
    return this.config.aa === true && process.platform === 'linux'
  }

  public pickPreferredTransport(): 'dongle' | 'aa' | null {
    const dongle = dongleConnected
    const native = this.hasNativeCandidate()
    if (!dongle && !native) return null

    // Explicit user override (from flip) wins, as long as the picked side is
    // still actually present.
    if (this.transportOverride === 'dongle' && dongle) return 'dongle'
    if (this.transportOverride === 'aa' && native) return 'aa'
    if (this.transportOverride) this.transportOverride = null

    const pref = this.getConnectionPreference()
    if (pref === 'dongle') return dongle ? 'dongle' : 'aa'
    if (pref === 'native') return native ? 'aa' : 'dongle'

    // 'auto' — sticky to whatever is already active
    const active = this.getActiveTransport()
    if (active === 'dongle' && dongle) return 'dongle'
    if (active === 'aa' && native) return 'aa'
    return dongle ? 'dongle' : 'aa'
  }

  public getActiveTransport(): 'dongle' | 'aa' | null {
    if (!this.started) return null
    return this.aaDriver ? 'aa' : 'dongle'
  }

  public getTransportState(): {
    active: 'dongle' | 'aa' | null
    dongleDetected: boolean
    nativeDetected: boolean
    preference: 'auto' | 'dongle' | 'native'
  } {
    return {
      active: this.getActiveTransport(),
      dongleDetected: dongleConnected,
      nativeDetected: this.hasNativeCandidate(),
      preference: this.getConnectionPreference()
    }
  }

  private emitTransportState(): void {
    this.emitProjectionEvent({
      type: 'transportState',
      payload: this.getTransportState()
    })
  }

  public async flipTransport(): Promise<{ ok: boolean; active: 'dongle' | 'aa' | null }> {
    const dongle = dongleConnected
    const native = this.hasNativeCandidate()
    if (!(dongle && native)) {
      // Flip only makes sense when both transports are present.
      return { ok: false, active: this.getActiveTransport() }
    }

    const current = this.getActiveTransport() ?? this.pickPreferredTransport()
    const target: 'dongle' | 'aa' = current === 'dongle' ? 'aa' : 'dongle'
    this.transportOverride = target

    if (this.started) {
      try {
        await this.stop()
      } catch (e) {
        console.warn('[ProjectionService] flipTransport: stop threw (ignored)', e)
      }
    }
    await this.autoStartIfNeeded()
    return { ok: true, active: this.getActiveTransport() }
  }

  /**
   * Pick the right driver for the upcoming session.
   * On switch, detach listeners from the old one, swap, attach to the new one.
   */
  private selectDriverFor(useAa: boolean): IPhoneDriver {
    if (useAa) {
      if (!this.aaDriver) {
        const aa = new AaDriver({
          onWillReenumerate: (ms) => this.expectPhoneReenumeration(ms)
        })
        this.aaDriver = aa
        aa.setHevcSupported(this.hevcSupported)
        aa.setVp9Supported(this.vp9Supported)
        aa.setAv1Supported(this.av1Supported)
        aa.setInitialNightMode(deriveInitialNightMode(this.config.appearanceMode))
        this.detachDriverListeners(this.dongleDriver)
        this.attachDriverListeners(aa)
        aa.on('connected', this.onAaConnected)
        aa.on('disconnected', this.onAaDisconnected)
        if (process.platform === 'linux') {
          this.openAaBtSubscription()
          this.populateAaBtPairedListInitial()
            .then(() => this.tryAutoConnect())
            .catch(() => {})
        }
      }
      return this.aaDriver
    }
    // useAa === false → return to dongle.
    if (this.aaDriver) {
      this.aaDriver.off('connected', this.onAaConnected)
      this.aaDriver.off('disconnected', this.onAaDisconnected)
      this.closeAaBtSubscription()
      this.detachDriverListeners(this.aaDriver)
      try {
        this.aaDriver.close()
      } catch (e) {
        console.warn('[ProjectionService] aaDriver.close threw on swap-out', e)
      }
      this.aaDriver = null
      this.attachDriverListeners(this.dongleDriver)
    }
    return this.dongleDriver
  }

  private async refreshAaBtPairedList(opts: { throwOnError?: boolean } = {}): Promise<void> {
    if (!this.aaDriver) return
    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch (e) {
      if (opts.throwOnError) throw e
      return
    }

    const entries: DevListEntry[] = devices.map((d) => ({
      id: d.mac,
      name: d.name || d.mac,
      type: 'AndroidAuto',
      source: 'host'
    }))
    const connected = devices.find((d) => d.connected)?.mac ?? ''
    if (connected && this.config.lastConnectedAaBtMac !== connected) {
      configEvents.emit('requestSave', { lastConnectedAaBtMac: connected })
    }

    const prev = isRecord(this.boxInfo) ? this.boxInfo : {}
    this.boxInfo = {
      ...prev,
      DevList: entries,
      btMacAddr: connected
    }
    this.emitDongleInfoIfChanged()

    // Also feed the renderer's bluetoothPairedDevices store. Format expected
    // by parseBluetoothPairedList: "<17-char MAC><name>\n" per device.
    if (this.webContents) {
      const raw = devices.length
        ? devices.map((d) => `${d.mac}${d.name ?? ''}`).join('\n') + '\n'
        : ''
      this.emitProjectionEvent({
        type: 'bluetoothPairedList',
        payload: raw
      })
    }
  }

  private async populateAaBtPairedListInitial(): Promise<void> {
    const totalTimeoutMs = 30_000
    const intervalMs = 2_000
    const deadline = Date.now() + totalTimeoutMs
    // If config has a remembered MAC, we expect at least that device to
    // surface in BlueZ. Treat an empty list as "still loading" until the
    // budget runs out, instead of accepting it as the final answer.
    const expectDevice = !!this.config.lastConnectedAaBtMac

    while (Date.now() < deadline) {
      if (!this.aaDriver) return
      try {
        await this.refreshAaBtPairedList({ throwOnError: true })
        const dev = (this.boxInfo as { DevList?: unknown[] } | undefined)?.DevList
        const isEmpty = !Array.isArray(dev) || dev.length === 0
        if (isEmpty && expectDevice) {
          await new Promise((r) => setTimeout(r, intervalMs))
          continue
        }
        return
      } catch {
        await new Promise((r) => setTimeout(r, intervalMs))
      }
    }
    console.warn(
      '[ProjectionService] aa-bt initial populate gave up after 30s — paired-device list may be empty until the next user action triggers a refresh'
    )
  }

  /** Pick a target from the paired list and fire a single Connect.
   *
   *  Preference: last-connected MAC (config), first trusted, first paired.
   *  Bails immediately if any device is already connected. */
  private async tryAutoConnect(): Promise<void> {
    if (!this.aaDriver) return

    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch {
      return
    }
    if (devices.some((d) => d.connected)) return

    const lastMac = this.config.lastConnectedAaBtMac
    const preferred = lastMac ? devices.find((d) => d.mac === lastMac) : null
    const trusted = devices.filter((d) => d.trusted)
    const target = preferred || trusted[0] || devices[0]
    if (!target) {
      console.log(
        `[ProjectionService] autoconnect: no candidate (paired=${devices.length}, lastMac=${lastMac ?? '∅'})`
      )
      return
    }

    const tag = preferred ? '[last]' : trusted.includes(target) ? '[trusted]' : '[first]'
    console.log(`[ProjectionService] autoconnect ${tag} → ${target.mac}`)
    try {
      const resp = await this.aaBtSock.connect(target.mac)
      if (!resp.ok) {
        console.log(`[ProjectionService] autoconnect: ${resp.error ?? 'failed'}`)
      }
    } catch (e) {
      console.log(`[ProjectionService] autoconnect threw: ${(e as Error).message}`)
    }
  }

  /** Open the long-lived aa-bt event subscription. Each device-changed event
   *  triggers a refresh. If the sock isn't bindable yet, retry once after
   *  the initial population finishes (~30 s budget). */
  private openAaBtSubscription(): void {
    if (this.aaBtSubscription) return
    const open = (): void => {
      if (!this.aaDriver) return
      this.aaBtSubscription = this.aaBtSock.subscribe(
        () => {
          this.refreshAaBtPairedList().catch(() => {})
        },
        () => {
          this.aaBtSubscription = null
          // Reopen if AaDriver is still active — typically means python
          // restarted or temporarily lost the connection.
          if (this.aaDriver) setTimeout(open, 1000)
        }
      )
    }
    open()
  }

  private closeAaBtSubscription(): void {
    if (!this.aaBtSubscription) return
    try {
      this.aaBtSubscription.close()
    } catch {
      /* already closed */
    }
    this.aaBtSubscription = null
  }

  // With preference='native', a synchronously detected dongle can otherwise
  // win the start-race against the async AOAP phone probe.
  private static readonly NATIVE_PROBE_POLL_MS = 250
  private static readonly NATIVE_PROBE_DEADLINE_MS = 3000
  private nativeProbeDeferred = false
  private nativeProbeDeadline = 0

  public async autoStartIfNeeded() {
    if (this.shuttingDown) return
    if (this.isStopping && this.stopPromise) {
      try {
        await this.stopPromise
      } catch {}
    }
    if (this.shuttingDown) return
    if (this.started || this.isStarting) return

    const target = this.pickPreferredTransport()
    if (target === null) return

    if (
      target === 'dongle' &&
      !this.transportOverride &&
      this.getConnectionPreference() === 'native'
    ) {
      const now = Date.now()
      if (!this.nativeProbeDeferred) {
        this.nativeProbeDeferred = true
        this.nativeProbeDeadline = now + ProjectionService.NATIVE_PROBE_DEADLINE_MS
        console.log(
          `[ProjectionService] preference=native — blocking dongle for up to ${ProjectionService.NATIVE_PROBE_DEADLINE_MS}ms`
        )
      }
      if (now < this.nativeProbeDeadline) {
        setTimeout(() => {
          this.autoStartIfNeeded().catch(console.error)
        }, ProjectionService.NATIVE_PROBE_POLL_MS)
        return
      }
      console.log(
        '[ProjectionService] preference=native — deadline hit, no native candidate, starting dongle'
      )
    }

    await this.start()
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
          voiceAssistant:
            typeof ext.voiceAssistantVolume === 'number' ? ext.voiceAssistantVolume : undefined,
          call: typeof ext.callVolume === 'number' ? ext.callVolume : undefined
        })

        this.audio.resetForSessionStart()

        this.dongleFwVersion = undefined
        if (isRecord(this.boxInfo)) {
          this.boxInfo = { ...this.boxInfo, btMacAddr: '' }
        }
        this.lastDongleInfoEmitKey = ''
        this.lastVideoWidth = undefined
        this.lastVideoHeight = undefined
        this.lastPluggedPhoneType = undefined
        this.lastClusterCodec = null
        this.aaPlaybackInferred = 1

        this.resetMediaSnapshot('session-start')
        this.resetNavigationSnapshot('session-start')

        const useAa = this.wantsAaDriver()
        const active = this.selectDriverFor(useAa)

        if (useAa) {
          // Two AA paths share the same driver: Wireless + Wired
          const wiredDevice = wiredPhoneDevice
          const aaDriver = active as AaDriver
          aaDriver.setWiredDevice(wiredDevice)

          if (wiredDevice) {
            const d = wiredDevice.deviceDescriptor
            console.log(
              `[ProjectionService] wired AA bring-up with device vid=0x${d.idVendor.toString(16)} pid=0x${d.idProduct.toString(16)}`
            )
          } else {
            console.log('[ProjectionService] wireless AA bring-up (no wired device)')
          }

          try {
            const ok = await aaDriver.start(this.config)
            this.started = Boolean(ok)
            if (this.started) {
              console.log(
                `[ProjectionService] started in AA mode (${wiredDevice ? 'wired' : 'wireless'})`
              )
            } else {
              console.warn(
                '[ProjectionService] aaDriver.start returned false — session not running'
              )
            }
          } catch (e) {
            console.warn('[ProjectionService] AA start failed', e)
            this.started = false
          }
          return
        }

        // Dongle (USB CPC200) path.
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

          await this.dongleDriver.initialise(asDomUSBDevice(webUsbDevice))

          if (this.pendingStartupConnectTarget) {
            this.dongleDriver.setPendingStartupConnectTarget(this.pendingStartupConnectTarget)
          } else {
            this.dongleDriver.clearPendingStartupConnectTarget()
          }

          await this.dongleDriver.start(this.config)

          this.pairTimeout = setTimeout(() => {
            this.dongleDriver.send(new SendCommand('wifiPair'))
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
        this.emitTransportState()
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
    this.nativeProbeDeferred = false
    this.nativeProbeDeadline = 0

    this.stopPromise = (async () => {
      this.clearTimeouts()

      try {
        const wc = this.webContents
        if (wc && !wc.isDestroyed()) {
          wc.send('projection-event', { type: 'unplugged' })
        }
      } catch (e) {
        console.warn('[ProjectionService] stop(): unplugged emit threw (ignored)', e)
      }

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

      if (this.aaDriver) {
        this.aaDriver.off('connected', this.onAaConnected)
        this.aaDriver.off('disconnected', this.onAaDisconnected)
        this.closeAaBtSubscription()
        this.detachDriverListeners(this.aaDriver)
        this.aaDriver = null
        this.attachDriverListeners(this.dongleDriver)
      }

      this.webUsbDevice = null
      this.audio.resetForSessionStop()

      this.started = false
      this.resetMediaSnapshot('session-stop')
      this.resetNavigationSnapshot('session-stop')

      this.dongleFwVersion = undefined
      if (isRecord(this.boxInfo)) {
        this.boxInfo = { ...this.boxInfo, btMacAddr: '' }
      }
      this.lastDongleInfoEmitKey = ''
      this.lastVideoWidth = undefined
      this.lastVideoHeight = undefined
      this.lastPluggedPhoneType = undefined
      this.aaPlaybackInferred = 2
    })().finally(() => {
      this.stopping = false
      this.isStopping = false
      this.stopPromise = null
      this.emitTransportState()
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

      this.emitProjectionEvent({
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

    this.emitProjectionEvent({ type: 'media-reset', reason })
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

    this.emitProjectionEvent({ type: 'navigation-reset', reason })
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
    extra?: Record<string, unknown>,
    targets?: WebContents[]
  ) {
    if (!data) return
    const wcs = targets ?? (this.webContents ? [this.webContents] : [])
    const isVideoChannel = channel === 'projection-video-chunk' || channel === 'cluster-video-chunk'
    const noTargets = wcs.length === 0

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

      if (noTargets && isVideoChannel) {
        // Buffer the chunk so it can be replayed once the renderer attaches.
        // Per-channel cap so a 60fps main stream can't push the cluster's
        // initial SPS/IDR out of the queue before the renderer connects.
        let q = this.earlyVideoQueues.get(channel)
        if (!q) {
          q = []
          this.earlyVideoQueues.set(channel, q)
        }
        q.push(envelope)
        if (q.length > ProjectionService.EARLY_QUEUE_MAX_PER_CHANNEL) {
          q.shift()
        }
      } else {
        for (const wc of wcs) {
          try {
            if (typeof wc.isDestroyed === 'function' && wc.isDestroyed()) continue
            wc.send(channel, envelope)
          } catch {
            // ignored: detached webContents
          }
        }
      }
      offset = end
    }
  }

  // Cluster video routing: list of webContents that should receive cluster
  // video chunks + resolution events, derived from settings.cluster.{main,
  // dash, aux}. Falls back to the bound main webContents when settings are
  // missing so the path stays compatible with existing tests / startup.
  private getClusterTargetWebContents(): WebContents[] {
    const cfg = this.config?.cluster ?? { main: true, dash: false, aux: false }
    const isAlive = (wc: WebContents | null | undefined): wc is WebContents => {
      if (!wc) return false
      try {
        return typeof wc.isDestroyed !== 'function' || !wc.isDestroyed()
      } catch {
        return true
      }
    }
    const out: WebContents[] = []
    if (cfg.main && isAlive(this.webContents)) out.push(this.webContents as WebContents)
    if (cfg.dash) {
      const w = getSecondaryWindow('dash')
      if (w && !w.isDestroyed() && isAlive(w.webContents)) out.push(w.webContents)
    }
    if (cfg.aux) {
      const w = getSecondaryWindow('aux')
      if (w && !w.isDestroyed() && isAlive(w.webContents)) out.push(w.webContents)
    }
    if (out.length === 0 && isAlive(this.webContents)) {
      out.push(this.webContents as WebContents)
    }
    return out
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
