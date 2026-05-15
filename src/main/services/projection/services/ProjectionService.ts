import { configEvents } from '@main/ipc/utils'
import { broadcastToSecondaryRenderers } from '@main/window/broadcast'
import { getSecondaryWindow } from '@main/window/secondaryWindows'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { DevListEntry, ExtraConfig } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import type { NavLocale } from '@shared/utils'
import { isClusterDisplayed, translateNavigation } from '@shared/utils'
import { app, WebContents } from 'electron'
import fs from 'fs'
import path from 'path'
import { type Device, usb, WebUSBDevice } from 'usb'
import { StatusFileWriter } from '../../status/StatusFileWriter'
import { AaBtSockClient } from '../driver/aa/AaBtSockClient'
import { AaDriver } from '../driver/aa/aaDriver'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import { ProjectionDriverManager } from '../drivers/ProjectionDriverManager'
import { type ProjectionIpcHost, registerProjectionIpc } from '../ipc'
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
  SoftwareVersion,
  Unplugged,
  VideoData
} from '../messages'
import { TransportArbiter } from '../transport/TransportArbiter'
import type { ConnectionPreference, Transport } from '../transport/types'
import {
  APP_START_TS,
  DEFAULT_MEDIA_DATA_RESPONSE,
  DEFAULT_NAVIGATION_DATA_RESPONSE,
  DEVTOOLS_IP_CANDIDATES
} from './constants'
import { FirmwareUpdateService } from './FirmwareUpdateService'
import { ProjectionAudio } from './ProjectionAudio'
import {
  type PendingStartupConnectTarget,
  PersistedMediaPayload,
  PersistedNavigationPayload
} from './types'
import { asDomUSBDevice } from './utils/asDomUSBDevice'
import { normalizeNavigationPayload } from './utils/normalizeNavigation'
import { readMediaFile } from './utils/readMediaFile'
import { readNavigationFile } from './utils/readNavigationFile'

type VolumeConfig = {
  audioVolume?: number
  navVolume?: number
  voiceAssistantVolume?: number
  callVolume?: number
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

export class ProjectionService {
  private readonly drivers: ProjectionDriverManager
  private readonly arbiter: TransportArbiter
  private get driver(): IPhoneDriver {
    return this.drivers.getActive()
  }
  private get aaDriver(): AaDriver | null {
    return this.drivers.getAa()
  }
  private get dongleDriver(): DongleDriver {
    return this.drivers.getDongle()
  }
  public getAaDriver(): AaDriver | null {
    return this.drivers.getAa()
  }
  public getDongleDriver(): DongleDriver {
    return this.drivers.getDongle()
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
  private readonly statusFile = new StatusFileWriter()

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
      this.arbiter.resetNativeProbeDefer()
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
      this.statusFile.setProjection(
        this.getActiveTransport(),
        msg.phoneType === PhoneType.CarPlay ? 'CarPlay' : 'AndroidAuto'
      )
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
      this.statusFile.setProjection(null, null)
      this.statusFile.setStreaming(false)
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
        this.statusFile.setStreaming(true)
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
        this.statusFile.applyAudioCommand(msg.command)
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
    this.drivers = new ProjectionDriverManager({
      handlers: {
        onMessage: (msg) => this.onDriverMessage(msg as Message),
        onFailure: () => this.onDriverFailure(),
        onTargetedConnect: () => this.onDriverTargetedConnect(),
        onVideoCodec: (c) => this.onDriverVideoCodec(c),
        onClusterVideoCodec: (c) => this.onDriverClusterVideoCodec(c)
      },
      onAaConnected: () => this.onAaConnected(),
      onAaDisconnected: () => this.onAaDisconnected(),
      onAaCreated: () => {
        if (process.platform === 'linux') {
          this.openAaBtSubscription()
          this.populateAaBtPairedListInitial()
            .then(() => this.tryAutoConnect())
            .catch(() => {})
        }
      },
      onAaReleased: () => this.closeAaBtSubscription(),
      getAaConfigSeed: () => ({
        hevcSupported: this.hevcSupported,
        vp9Supported: this.vp9Supported,
        av1Supported: this.av1Supported,
        initialNightMode: deriveInitialNightMode(this.config.appearanceMode)
      }),
      onPhoneReenumerate: (ms) => this.expectPhoneReenumeration(ms)
    })

    this.arbiter = new TransportArbiter({
      getPreference: () => this.getConnectionPreference(),
      isAaEligible: () => this.config.aa === true && process.platform === 'linux',
      getActiveTransport: () => this.getActiveTransport(),
      isDongleSessionActive: () => this.started && !this.drivers.getAa(),
      isWiredAaSessionActive: () => this.started && this.drivers.getAa()?.isWiredMode() === true,
      onChange: () => this.emitTransportState(),
      onShouldStop: async () => {
        await this.stop()
      },
      onShouldAutoStart: () => {
        this.autoStartIfNeeded().catch(console.error)
      }
    })

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

    const ipcHost: ProjectionIpcHost = {
      start: () => this.start(),
      stop: () => this.stop(),
      pickPreferredTransport: () => this.arbiter.pickPreferred(),
      flipTransport: () => this.flipTransport(),
      getTransportState: () => this.getTransportState(),
      applyCodecCapabilities: (caps) => this.applyCodecCapabilities(caps),
      send: (msg) => this.driver.send(msg),
      isUsingDongle: () => this.driver instanceof DongleDriver,
      isUsingAa: () => this.drivers.getAa() !== null,
      isStarted: () => this.started,
      hasWebUsbDevice: () => this.webUsbDevice !== null,
      sendBluetoothPairedList: (text) => this.dongleDriver.sendBluetoothPairedList(text),
      connectAaBt: (mac) => this.aaBtSock.connect(mac),
      removeAaBt: (mac) => this.aaBtSock.remove(mac),
      refreshAaBtPaired: () => {
        this.refreshAaBtPairedList().catch(() => {})
      },
      getBoxInfo: () => this.boxInfo,
      setPendingStartupConnectTarget: (t) => {
        this.pendingStartupConnectTarget = t
      },
      getConfig: () => this.config,
      setClusterRequested: (v) => {
        this.clusterRequested = v
      },
      resetLastClusterVideoSize: () => {
        this.lastClusterVideoWidth = undefined
        this.lastClusterVideoHeight = undefined
      },
      getLastClusterCodec: () => this.lastClusterCodec,
      getClusterTargetWebContents: () => this.getClusterTargetWebContents(),
      uploadIcons: () => this.uploadIcons(),
      getDevToolsUrlCandidates: () => this.getDevToolsUrlCandidates(),
      reloadConfigFromDisk: () => this.reloadConfigFromDisk(),
      getFirmware: () => this.firmware,
      getApkVer: () => this.getApkVer(),
      getDongleFwVersion: () => this.dongleFwVersion,
      emitProjectionEvent: (p) => this.emitProjectionEvent(p),
      setAudioStreamVolume: (s, v) => this.audio.setStreamVolume(s, v),
      setAudioVisualizerEnabled: (e) => this.audio.setVisualizerEnabled(e)
    }
    registerProjectionIpc(ipcHost)

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

  public markDongleConnected(connected: boolean): void {
    this.arbiter.markDongleConnected(connected)
    this.statusFile.setUsbState(this.arbiter.isPhoneConnected(), connected)
  }

  public markPhoneConnected(connected: boolean, device?: Device): void {
    this.arbiter.markPhoneConnected(connected, device)
    this.statusFile.setUsbState(connected, this.arbiter.getSnapshot().dongleDetected)
  }

  public getWiredPhoneDevice(): Device | null {
    return this.arbiter.getPhoneDevice()
  }

  public isWiredPhoneConnected(): boolean {
    return this.arbiter.isPhoneConnected()
  }

  public expectPhoneReenumeration(durationMs: number): void {
    this.arbiter.expectPhoneReenumeration(durationMs)
  }

  public isExpectingPhoneReenumeration(): boolean {
    return this.arbiter.isExpectingPhoneReenumeration()
  }

  private getConnectionPreference(): ConnectionPreference {
    const raw = (this.config as { connectionPreference?: unknown }).connectionPreference
    return raw === 'dongle' || raw === 'native' ? raw : 'auto'
  }

  public pickPreferredTransport(): Transport | null {
    return this.arbiter.pickPreferred()
  }

  public getActiveTransport(): Transport | null {
    if (!this.started) return null
    return this.aaDriver ? 'aa' : 'dongle'
  }

  public getTransportState() {
    return this.arbiter.getSnapshot()
  }

  private emitTransportState(): void {
    this.emitProjectionEvent({
      type: 'transportState',
      payload: this.arbiter.getSnapshot()
    })
  }

  public async flipTransport(): Promise<{ ok: boolean; active: Transport | null }> {
    const { ok, target } = this.arbiter.prepareFlip()
    if (!ok) return { ok: false, active: target }

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

  private selectDriverFor(transport: Transport): IPhoneDriver {
    return this.drivers.selectFor(transport)
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

  public async autoStartIfNeeded() {
    if (this.shuttingDown) return
    if (this.isStopping && this.stopPromise) {
      try {
        await this.stopPromise
      } catch {}
    }
    if (this.shuttingDown) return
    if (this.started || this.isStarting) return

    const decision = this.arbiter.decideNextStart()
    if (decision.kind === 'none') return
    if (decision.kind === 'defer') {
      setTimeout(() => {
        this.autoStartIfNeeded().catch(console.error)
      }, decision.retryMs)
      return
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

        const target: Transport = this.arbiter.pickPreferred() === 'aa' ? 'aa' : 'dongle'
        const active = this.selectDriverFor(target)
        const useAa = target === 'aa'

        if (useAa) {
          // Two AA paths share the same driver: Wireless + Wired
          const wiredDevice = this.arbiter.getPhoneDevice()
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
    this.arbiter.resetNativeProbeDefer()

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

      this.drivers.releaseAa()

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
