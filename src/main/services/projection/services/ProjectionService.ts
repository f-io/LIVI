import { configEvents } from '@main/ipc/utils'
import { SystemSound } from '@main/services/audio'
import { broadcastToSecondaryRenderers } from '@main/window/broadcast'
import { getSecondaryWindow } from '@main/window/secondaryWindows'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { Config, DevListEntry } from '@shared/types'
import { PhoneWorkMode } from '@shared/types'
import { isInputCommand } from '@shared/types/InputCommand'
import type { ClusterScreen, NavLocale } from '@shared/utils'
import { aaContentArea, clusterTargetScreens, isClusterDisplayed } from '@shared/utils'
import { app, WebContents, webContents } from 'electron'
import fs from 'fs'
import path from 'path'
import {
  type AudioDeviceMonitorHandle,
  startAudioDeviceMonitor
} from '../../audio/AudioDeviceEnumerator'
import { StatusFileWriter } from '../../status/StatusFileWriter'
import { GstVideo, type GstVideoCodec, probeGstCodecs } from '../../video/GstVideo'
import { AaBtSockClient } from '../driver/aa/AaBtSockClient'
import type { AaSession } from '../driver/aa/AaSession'
import type { CpManager } from '../driver/cp/CpManager'
import type { CpSession } from '../driver/cp/CpSession'
import { HelperSupervisor } from '../driver/helper/helperSupervisor'
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
  GnssData,
  MediaData,
  MediaType,
  type Message,
  MessageType,
  NavigationData,
  PhoneType,
  Plugged,
  SoftwareVersion,
  VideoData
} from '../messages'
import { TransportArbiter } from '../transport/TransportArbiter'
import type { Transport } from '../transport/types'
import { CodecCapabilityService } from './CodecCapabilityService'
import {
  APP_START_TS,
  DEFAULT_MEDIA_DATA_RESPONSE,
  DEFAULT_NAVIGATION_DATA_RESPONSE,
  DEVTOOLS_IP_CANDIDATES
} from './constants'
import { DeviceController } from './DeviceController'
import { DeviceRegistry, type DeviceView } from './DeviceRegistry'
import { FirmwareUpdateService } from './FirmwareUpdateService'
import { MediaStore } from './MediaStore'
import { NavStore } from './NavStore'
import { ProjectionAudio } from './ProjectionAudio'
import { type ProjectionSession, SessionManager, type SessionTransport } from './SessionManager'
import { type PendingStartupConnectTarget, type ProjectionEvent } from './types'
import { isPhoneLikeCod } from './utils/isPhoneLikeCod'

type Device = USBDevice

const APPLE_VENDOR_ID = 0x05ac

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

// Capped exponential backoff for a failed session bring-up (transient USB busy, phone locked).
// The retry stops on its own once the phone detaches and resets on a successful start.
const START_RETRY_BASE_MS = 1000
const START_RETRY_CAP_MS = 15000

export class ProjectionService {
  private readonly drivers: ProjectionDriverManager
  private readonly arbiter: TransportArbiter
  private get driver(): IPhoneDriver {
    return this.drivers.getActive()
  }
  private get dongleDriver(): DongleDriver {
    return this.drivers.getDongle()
  }
  private activeAaSession(): AaSession | null {
    const a = this.sessions.active()
    return a?.protocol === 'androidauto' ? (a.driver as AaSession) : null
  }
  private isActiveAaWired(): boolean {
    const a = this.sessions.active()
    return a?.protocol === 'androidauto' && a.transport === 'usb'
  }
  private isActiveCpWired(): boolean {
    const a = this.sessions.active()
    return a?.protocol === 'carplay' && a.transport === 'usb'
  }
  public getAaDriver(): AaSession | null {
    return this.activeAaSession()
  }
  public getDongleDriver(): DongleDriver {
    return this.drivers.getDongle()
  }
  public getCpDriver(): CpManager | null {
    return this.drivers.getCpManager()
  }
  private readonly codecCaps = new CodecCapabilityService((codec, supported) => {
    if (codec === 'hevc') {
      this.drivers.setAaHevcSupported(supported)
      this.drivers.setCpHevcSupported(supported)
    } else if (codec === 'vp9') {
      this.drivers.setAaVp9Supported(supported)
      this.drivers.setCpVp9Supported(supported)
    } else {
      this.drivers.setAaAv1Supported(supported)
      this.drivers.setCpAv1Supported(supported)
    }
  })

  private readonly mediaStore = new MediaStore({
    emit: (p) => this.emitProjectionEvent(p),
    getPlaybackInferred: () => this.aaPlaybackInferred,
    getLastPhoneType: () => this.lastPluggedPhoneType
  })
  private readonly navStore = new NavStore({
    emit: (p) => this.emitProjectionEvent(p),
    getLanguage: () => this.config.language,
    isStarted: () => this.started
  })
  private webContents: WebContents | null = null
  private config: Config = DEFAULT_CONFIG as Config
  private startRetryTimer: NodeJS.Timeout | null = null
  private startRetryAttempt = 0

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
  private gstVideo: GstVideo | null = null
  private gstVideoCodec: GstVideoCodec = 'h264'
  private gstVideoVisible = true
  private videoCrop: {
    cropL: number
    cropT: number
    visW: number
    visH: number
    tierW: number
    tierH: number
  } | null = null
  private gstVideoClusters = new Map<ClusterScreen, GstVideo>()
  private gstVideoClusterCodec: GstVideoCodec = 'h264'
  private lastMainCodecByDriver = new Map<IPhoneDriver, GstVideoCodec>()
  private lastClusterCodecByDriver = new Map<IPhoneDriver, GstVideoCodec>()
  private clusterVisible = false
  private clusterStreamActive: boolean | null = null
  private dongleFwVersion?: string
  private boxInfo?: unknown
  private hostDevList: DevListEntry[] = []
  private dongleDevList: DevListEntry[] = []
  private dongleConnectedMac = ''
  private hostPairedRaw = ''
  private donglePairedRaw = ''
  private lastDongleInfoEmitKey = ''
  private lastAudioMetaEmitKey = ''
  private firmware = new FirmwareUpdateService()
  private readonly aaBtSock = new AaBtSockClient()
  private aaBtSubscription: { close: () => void } | null = null
  private aaBtNameByMac = new Map<string, string>()
  private connectedAaBtMac = ''
  private readonly aaBtMacByInstance = new Map<string, string>()
  private readonly aaSerialByInstance = new Map<string, string>()
  private audioMonitor: AudioDeviceMonitorHandle | null = null
  private readonly statusFile = new StatusFileWriter()

  private helperSupervisor: HelperSupervisor | null = null
  private btEnableKey = ''
  private btAaWireless = false
  private btCpWireless = false
  private readonly deviceRegistry = new DeviceRegistry()
  private sessions!: SessionManager
  private readonly deviceController = new DeviceController({
    deviceRegistry: this.deviceRegistry,
    sessions: () => this.sessions,
    getDongleSession: () => this.sessions.byDriver(this.drivers.getDongle()),
    aaBtSock: this.aaBtSock,
    getAaBtName: (mac) => this.aaBtNameByMac.get(mac),
    getAaBtMac: () => this.connectedAaBtMac,
    getDongleConnectedMac: () => this.dongleConnectedMac,
    getDongleDevList: () => this.dongleDevList,
    emit: (p) => this.emitProjectionEvent(p),
    autoConnect: () => this.config.autoConn !== false,
    pushReconnectTargets: (targets) => {
      this.drivers
        .getCpManager()
        ?.helper.sendReconnectTargets(targets)
        .catch(() => {})
    },
    pushWiredPhones: (ids) => {
      this.aaBtSock.setWiredPhones(ids).catch(() => {})
    }
  })
  private aaBtActive = false
  private cpActive = false
  private wirelessPhoneInRange = false
  private btInitialQueryDone = false
  private isSwitching = false

  private aaTransport(session: AaSession): SessionTransport {
    return session.isWiredMode() ? 'usb' : 'wifi'
  }
  private maybeAutoActivate(s: ProjectionSession): void {
    if (!this.sessions.active()) this.sessions.activate(s.index)
  }
  private readonly onAaConnected = (session: AaSession): void => {
    this.refreshAaBtPairedList().catch(() => {})
    this.maybeAutoActivate(
      this.sessions.upsert(session, 'androidauto', this.aaTransport(session), {})
    )
    this.onPhoneConnected(PhoneType.AndroidAuto)
  }
  private readonly onAaDisconnected = (session: AaSession): void => {
    this.refreshAaBtPairedList().catch(() => {})
    const closed = this.sessions.byDriver(session)
    this.sessions.closeByDriver(session)
    if (closed) {
      this.deviceRegistry.clearPresence(closed.device)
    }
    this.lastMainCodecByDriver.delete(session)
    this.lastClusterCodecByDriver.delete(session)
    // Only tear down the shared audio + media when no session is left active. A held phone
    // dropping must not reset the ACTIVE phone's audio/UI; the active-session change (or
    // teardown-to-idle) owns that.
    if (!this.sessions.active()) {
      try {
        this.audio.resetForSessionStop()
      } catch (e) {
        console.warn('[ProjectionService] audio reset on AA disconnect threw (ignored)', e)
      }
      this.mediaStore.reset('aa-session-end')
    }
    this.onPhoneDisconnected()
  }

  private readonly onCpConnected = (session: CpSession): void => {
    this.maybeAutoActivate(
      this.sessions.upsert(session, 'carplay', 'wifi', {
        controllerId: session.getControllerId() ?? undefined
      })
    )
    this.onPhoneConnected(PhoneType.CarPlay)
  }
  private readonly onCpDisconnected = (session: CpSession): void => {
    const closed = this.sessions.byDriver(session)
    this.sessions.closeByDriver(session)
    if (closed) {
      this.deviceRegistry.clearPresence(closed.device)
    }
    this.lastMainCodecByDriver.delete(session)
    this.lastClusterCodecByDriver.delete(session)
    this.onPhoneDisconnected()
  }

  // Registry-level helper presence (hostapd wifi + Bonjour/carkit device): tracks
  // phones in range independent of any projecting session.
  private onCpHelperPresence(p: Record<string, unknown>): void {
    const ip = typeof p.ip === 'string' ? p.ip : ''
    if (p.kind === 'wifi') {
      const wifiMac = typeof p.wifiMac === 'string' ? p.wifiMac : undefined
      const up = p.connected === true
      this.deviceRegistry.noteLink({ wifiMac, ip: ip || undefined }, 'wifi', up)
      if (!up) this.sessions.closeByDeviceOnTransport({ wifiMac, ip: ip || undefined }, 'wifi')
      return
    }
    if (p.kind === 'device') {
      const btMac = typeof p.btMac === 'string' ? p.btMac : undefined
      const usbUdid = typeof p.usbUdid === 'string' ? p.usbUdid : undefined
      this.deviceRegistry.noteDevice({
        btMac,
        ip: ip || undefined,
        usbUdid,
        name: typeof p.name === 'string' ? p.name : undefined,
        protocol: 'carplay',
        transport: usbUdid ? 'usb' : 'wifi'
      })
    }
    if (p.kind === 'device-gone') {
      const usbUdid = typeof p.usbUdid === 'string' ? p.usbUdid : undefined
      if (!usbUdid) return
      this.sessions.closeByDeviceOnTransport({ usbUdid }, 'usb')
    }
  }

  private onCpPresence(session: CpSession, p: Record<string, unknown>): void {
    const ip = typeof p.ip === 'string' ? p.ip : ''
    switch (p.kind) {
      case 'device': {
        const btMac = typeof p.btMac === 'string' ? p.btMac : undefined
        const usbUdid = typeof p.usbUdid === 'string' ? p.usbUdid : undefined
        const wifiMacRaw = typeof p.wifiMac === 'string' ? p.wifiMac : undefined
        // Wiredness follows the phone's udid, sticky across a later wifi-only device-info presence.
        const wired =
          !!usbUdid ||
          this.sessions.byIdentity('carplay', {
            btMac,
            wifiMac: wifiMacRaw,
            usbUdid,
            ip: ip || undefined
          })?.transport === 'usb'
        const wifiMac = wired ? undefined : wifiMacRaw
        this.deviceRegistry.noteDevice({
          btMac,
          wifiMac,
          ip: ip || undefined,
          usbUdid,
          name: typeof p.name === 'string' ? p.name : undefined,
          model: typeof p.model === 'string' ? p.model : undefined,
          protocol: 'carplay',
          transport: wired ? 'usb' : 'wifi'
        })
        // A session born at iAP2 identification (socket-less metadata driver) is taken over
        // by this AirPlay transport: hand it the identity + accumulated media/nav, then drop
        // the placeholder, so the phone stays ONE session, not two.
        const born = this.sessions.byIdentity('carplay', {
          btMac,
          wifiMac,
          usbUdid,
          ip: ip || undefined
        })
        if (born && born.driver !== session) {
          const placeholder = born.driver
          this.sessions.reassignDriver(placeholder, session)
          void placeholder.close()
        }
        this.maybeAutoActivate(
          this.sessions.upsert(session, 'carplay', 'wifi', {
            btMac,
            wifiMac,
            usbUdid,
            ip: ip || undefined
          })
        )
        break
      }
      case 'active': {
        const s = this.sessions.byDriver(session)
        if (s) this.maybeAutoActivate(s)
        break
      }
      case 'status': {
        const ids = this.sessions.byDriver(session)?.device ?? {}
        this.deviceRegistry.noteStatus(ids, {
          batteryLevel: typeof p.batteryLevel === 'number' ? p.batteryLevel : undefined,
          batteryCharging: typeof p.batteryCharging === 'boolean' ? p.batteryCharging : undefined,
          signalStrength: typeof p.signalStrength === 'number' ? p.signalStrength : undefined,
          carrierName: typeof p.carrierName === 'string' ? p.carrierName : undefined
        })
        break
      }
    }
  }

  private onAaPresence(session: AaSession, p: Record<string, unknown>): void {
    const ip = typeof p.ip === 'string' ? p.ip : ''
    if (p.kind === 'status') {
      const ids = this.sessions.byDriver(session)?.device ?? {}
      this.deviceRegistry.noteStatus(ids, {
        batteryLevel: typeof p.batteryLevel === 'number' ? p.batteryLevel : undefined,
        batteryCritical: typeof p.batteryCritical === 'boolean' ? p.batteryCritical : undefined,
        batteryTimeRemaining:
          typeof p.batteryTimeRemaining === 'number' ? p.batteryTimeRemaining : undefined,
        signalStrength: typeof p.signalStrength === 'number' ? p.signalStrength : undefined
      })
      return
    }
    if (p.kind !== 'device') return
    const wired = session.isWiredMode()
    const instanceId = typeof p.instanceId === 'string' ? p.instanceId : undefined
    const wifiMac = !wired && typeof p.wifiMac === 'string' ? p.wifiMac : undefined
    const btMac = !wired && instanceId ? this.aaBtMacByInstance.get(instanceId) : undefined
    const usbSerial =
      session.usbSerial() || (instanceId ? this.aaSerialByInstance.get(instanceId) : undefined)
    this.deviceRegistry.noteDevice({
      btMac,
      instanceId,
      usbSerial,
      wifiMac,
      name: typeof p.name === 'string' && p.name ? p.name : undefined,
      model: typeof p.model === 'string' && p.model ? p.model : undefined,
      ip: ip || undefined,
      protocol: 'androidauto',
      transport: wired ? 'usb' : 'wifi'
    })
    this.maybeAutoActivate(
      this.sessions.upsert(session, 'androidauto', this.aaTransport(session), {
        btMac,
        instanceId,
        usbSerial,
        wifiMac,
        ip: ip || undefined
      })
    )
  }

  private disposeGstPlanes(): void {
    this.gstVideo?.dispose()
    this.gstVideo = null
    this.gstVideoCodec = 'h264'
    for (const plane of this.gstVideoClusters.values()) plane.dispose()
    this.gstVideoClusters.clear()
    this.gstVideoClusterCodec = 'h264'
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
  private readonly clusterRequestedBy = new Set<number>()
  private lastClusterCodec: 'h264' | 'h265' | 'vp9' | 'av1' | null = null

  // Per-channel buffers for video chunks that arrive from the phone before
  // the renderer is attached.
  private earlyVideoQueues: Map<string, Array<Record<string, unknown>>> = new Map()
  private static readonly EARLY_QUEUE_MAX_PER_CHANNEL = 256
  private lastPluggedPhoneType?: PhoneType
  private aaPlaybackInferred: 1 | 2 = 1
  private pendingStartupConnectTarget: PendingStartupConnectTarget | null = null

  private audio: ProjectionAudio
  private systemSound = new SystemSound(() => this.config)

  private readonly onConfigChanged = (next: Config) => {
    if (this.shuttingDown) return
    const prev = this.config
    this.config = { ...this.config, ...next }

    const prevClusterActive = isClusterDisplayed(prev)
    const nextClusterActive = isClusterDisplayed(this.config)
    const clusterToggled = prevClusterActive !== nextClusterActive

    if (clusterToggled && !nextClusterActive) {
      this.clusterRequestedBy.clear()
      this.lastClusterCodec = null
      this.lastClusterVideoWidth = undefined
      this.lastClusterVideoHeight = undefined
    }

    // Drop cluster planes for screens no longer targeted (re-spawn on demand)
    const nextScreens = new Set(clusterTargetScreens(this.config))
    for (const [screen, plane] of this.gstVideoClusters) {
      if (!nextScreens.has(screen)) {
        plane.dispose()
        this.gstVideoClusters.delete(screen)
      }
    }
    this.syncClusterStreamFocus()

    // Seed AA's initial NIGHT_MODE
    if (next.appearanceMode !== prev?.appearanceMode) {
      this.drivers.setAaInitialNightMode(deriveInitialNightMode(next.appearanceMode))
    }

    if (
      (typeof next.wirelessAaEnabled === 'boolean' &&
        next.wirelessAaEnabled !== prev?.wirelessAaEnabled) ||
      (typeof next.wirelessCpEnabled === 'boolean' &&
        next.wirelessCpEnabled !== prev?.wirelessCpEnabled)
    ) {
      this.syncHelperSupervisor()
      this.emitTransportState()
    }

    const outChanged = next.audioOutputDevice !== prev?.audioOutputDevice
    const inChanged = next.audioInputDevice !== prev?.audioInputDevice
    if (outChanged || inChanged) {
      this.audio.onAudioDeviceChanged()
      if (outChanged) this.systemSound.onDeviceChanged()
      this.connectConfiguredAudioDevices().catch(() => {})
    }
  }

  private syncHelperSupervisor(): void {
    const linux = process.platform === 'linux'
    const wantAaWireless = linux && this.config.wirelessAaEnabled === true
    const wantCpWireless = linux && this.config.wirelessCpEnabled === true
    // Wired CP (carkit) always runs on Linux, like wired AA. Wireless (Wi-Fi AP +
    // BT profiles) is toggled live over the control socket; the helper process never
    // restarts for a wireless config change, so wired sessions survive the toggle.
    const wantCp = linux
    const want = wantAaWireless || wantCp
    const enableKey = want ? 'h' : ''
    // The spawn env only carries the initial AA/CP wireless state; later changes go
    // over the control socket.
    const restarting = want && (!this.helperSupervisor || this.btEnableKey !== enableKey)

    if (restarting) {
      if (this.helperSupervisor) {
        const old = this.helperSupervisor
        this.helperSupervisor = null
        old.stop().catch(() => {})
      }
      const sup = new HelperSupervisor({ maxRestarts: 5 })
      sup.on('stdout', (line) => console.log(`[helper] ${line}`))
      sup.on('stderr', (line) => console.warn(`[helper!] ${line}`))
      sup.on('error', (err) => console.warn(`[bt] supervisor error: ${err.message}`))
      this.helperSupervisor = sup
      this.btEnableKey = enableKey
      console.log(
        `[ProjectionService] starting unified BT supervisor (aaWireless=${wantAaWireless} cpWireless=${wantCpWireless})`
      )
      sup.start(this.config)
    } else if (!want && this.helperSupervisor) {
      console.log('[ProjectionService] stopping unified BT supervisor')
      const sup = this.helperSupervisor
      this.helperSupervisor = null
      this.btEnableKey = ''
      sup.stop().catch((e) => console.warn('[ProjectionService] bt supervisor stop threw', e))
    } else if (this.helperSupervisor && this.btAaWireless !== wantAaWireless) {
      console.log(`[ProjectionService] toggling wireless AA live (aaWireless=${wantAaWireless})`)
      this.drivers.getCpManager()?.setAaWireless(wantAaWireless)
    }
    this.btAaWireless = wantAaWireless

    if (wantAaWireless && !this.aaBtActive) {
      this.aaBtActive = true
      this.drivers.startAaWireless()
      this.openAaBtSubscription()
      this.populateAaBtPairedListInitial()
        .then(() => {
          this.emitTransportState()
          this.connectConfiguredAudioDevices().catch(() => {})
        })
        .catch(() => {})
    } else if (!wantAaWireless && this.aaBtActive) {
      this.aaBtActive = false
      this.closeAaBtSubscription()
      this.setWirelessPhoneInRange(false)
      this.btInitialQueryDone = false
      this.drivers.stopAaWireless()
    }

    // CpManager owns the CarPlay :7000 listener + the helper event feed, which WIRED CP
    // needs as much as wireless (the phone reaches :7000 over the USB link-local too), so it
    // runs whenever wired CP is possible (wantCp) — not gated on cpWireless.
    if (wantCp && !this.cpActive) {
      this.cpActive = true
      this.drivers.startCp()
    } else if (!wantCp && this.cpActive) {
      this.cpActive = false
      void this.drivers.releaseCp()
    }
    // cpWireless only toggles the wireless CP BT profile live over the control socket.
    if (this.cpActive && !restarting && this.btCpWireless !== wantCpWireless) {
      console.log(`[ProjectionService] toggling wireless CP live (cpWireless=${wantCpWireless})`)
      this.drivers.getCpManager()?.setCpWireless(wantCpWireless)
    }
    this.btCpWireless = wantCpWireless
  }

  private setWirelessPhoneInRange(value: boolean): void {
    if (this.wirelessPhoneInRange === value) return
    const becameAvailable = !this.wirelessPhoneInRange && value
    this.wirelessPhoneInRange = value
    this.emitTransportState()
    if (becameAvailable) this.autoStartIfNeeded().catch(console.error)
  }

  // Single emit point for `projection-event`
  private emitProjectionEvent(payload: ProjectionEvent): void {
    this.webContents?.send('projection-event', payload)
    broadcastToSecondaryRenderers('projection-event', payload)
  }

  // Reflects the current HEVC decode capability seeded into each AA session
  public getHevcSupported(): boolean {
    return this.codecCaps.hevc
  }

  private handleSoftwareVersion(msg: SoftwareVersion): void {
    this.dongleFwVersion = msg.version
    this.emitDongleInfoIfChanged()
  }

  private handleBoxInfo(msg: BoxInfo): void {
    const settings = msg.settings as { DevList?: Array<Record<string, unknown>> }
    if (this.setDongleDevListFromSettings(settings)) {
      settings.DevList = this.mergedDevList() as unknown as Array<Record<string, unknown>>
    }
    const rawBtMac = (msg.settings as { btMacAddr?: unknown }).btMacAddr
    if (typeof rawBtMac === 'string' && rawBtMac.trim()) {
      this.dongleConnectedMac = rawBtMac.trim()
    }
    this.boxInfo = mergePreferExisting(this.boxInfo, msg.settings)
    this.emitDongleInfoIfChanged()
    this.deviceController.emitDevices()
  }

  private setDongleDevListFromSettings(settings: {
    DevList?: Array<Record<string, unknown>>
  }): boolean {
    if (!Array.isArray(settings.DevList)) return false
    this.dongleDevList = settings.DevList.map((entry) => ({
      ...(entry as DevListEntry),
      source: 'dongle' as const
    }))
    return true
  }

  // Dongle lifecycle over always-on driver events (not the routed 'message' path),
  // so a held dongle still appears + is selectable in the picker while native sessions run.
  private onDonglePhoneConnected(): void {
    this.maybeAutoActivate(this.sessions.upsert(this.drivers.getDongle(), 'dongle', 'usb', {}))
    this.deviceController.emitDevices()
  }

  private onDonglePhoneDisconnected(): void {
    const dongle = this.drivers.getDongle()
    const hadOther = this.sessions.all().some((s) => s.driver !== dongle)
    this.sessions.closeByDriver(dongle)
    this.dongleDevList = []
    this.donglePairedRaw = ''
    this.dongleConnectedMac = ''
    if (isRecord(this.boxInfo)) {
      this.boxInfo = { ...this.boxInfo, btMacAddr: '' }
    }
    this.emitProjectionEvent({
      type: 'dongleInfo',
      payload: {
        dongleFwVersion: this.dongleFwVersion,
        boxInfo: this.boxInfo
      }
    })
    if (hadOther) this.deviceController.emitDevices()
    else this.onPhoneDisconnected()
  }

  private onDongleInfo(info: { boxInfo?: unknown }): void {
    const settings = info.boxInfo as { DevList?: Array<Record<string, unknown>> } | undefined
    if (settings && this.setDongleDevListFromSettings(settings)) {
      this.deviceController.emitDevices()
    }
  }

  private handleGnssData(msg: GnssData): void {
    this.emitProjectionEvent({
      type: 'gnss',
      payload: {
        text: msg.text
      }
    })
  }

  private handleBluetoothPairedList(msg: BluetoothPairedList): void {
    this.donglePairedRaw = msg.data
    this.emitCombinedBtPairedList()
  }

  private handleBoxUpdateProgress(msg: BoxUpdateProgress): void {
    // 0xb1 payload: int32 progress
    this.emitProjectionEvent({
      type: 'fwUpdate',
      stage: 'upload:progress',
      progress: msg.progress
    })
  }

  private handleBoxUpdateState(msg: BoxUpdateState): void {
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

      this.driver.requestKeyframe?.()
    }
  }

  private handlePlugged(msg: Plugged): void {
    this.onPhoneConnected(msg.phoneType)
    if (!this.started && !this.isStarting && this.getActiveTransport() !== 'cp') {
      this.start().catch(() => {})
    }
  }

  private onPhoneConnected(phoneType: PhoneType): void {
    this.clearTimeouts()
    this.lastPluggedPhoneType = phoneType
    this.aaPlaybackInferred = 1
    this.lastVideoWidth = undefined
    this.lastVideoHeight = undefined
    this.lastClusterVideoWidth = undefined
    this.lastClusterVideoHeight = undefined

    const nextPhoneWorkMode =
      phoneType === PhoneType.CarPlay ? PhoneWorkMode.CarPlay : PhoneWorkMode.Android

    try {
      configEvents.emit('requestSave', { lastPhoneWorkMode: nextPhoneWorkMode })
    } catch (e) {
      console.warn('[ProjectionService] failed to persist lastPhoneWorkMode (ignored)', e)
    }

    this.emitProjectionEvent({ type: 'plugged', phoneType })
    this.statusFile.setProjection(
      this.getActiveTransport(),
      phoneType === PhoneType.CarPlay ? 'CarPlay' : 'AndroidAuto'
    )
    for (const fn of this.pluggedHooks) {
      try {
        fn(phoneType)
      } catch (e) {
        console.warn('[ProjectionService] plugged hook threw (ignored)', e)
      }
    }
  }

  private onPhoneDisconnected(): void {
    this.clearTimeouts()
    this.lastPluggedPhoneType = undefined
    this.aaPlaybackInferred = 1
    // A held phone dropping must not blank the ACTIVE phone's projection: clear the
    // UI/status/nav only when no session is left active (onActiveSessionChanged /
    // teardownToIdle drives the active-session case).
    if (!this.sessions.active()) {
      this.emitProjectionEvent({ type: 'unplugged' })
      this.statusFile.setProjection(null, null)
      this.statusFile.setStreaming(false)
      this.navStore.reset('phone-disconnect')
    }
    this.deviceController.emitDevices()
  }

  private handleVideoData(msg: VideoData): void {
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
        const active = this.sessions.active()
        if (active) {
          active.video.cluster.width = w
          active.video.cluster.height = h
        }
        for (const wc of clusterTargets) {
          if (!wc.isDestroyed()) wc.send('cluster-video-resolution', { width: w, height: h })
        }
        for (const plane of this.gstVideoClusters.values()) this.applyClusterCrop(plane)
      }

      if (msg.data) this.pushGstVideoCluster(msg.data)
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
      const active = this.sessions.active()
      if (active) {
        active.video.main.width = w
        active.video.main.height = h
      }
      this.updateVideoCrop()

      this.emitProjectionEvent({
        type: 'resolution',
        payload: { width: w, height: h }
      })
    }

    if (msg.data) this.pushGstVideo(msg.data)
  }

  private handleAudioData(msg: AudioData): void {
    this.audio.handleAudioData(msg)

    if (msg.command != null) {
      this.statusFile.applyAudioCommand(msg.command)
      if (this.lastPluggedPhoneType === PhoneType.AndroidAuto) {
        if (msg.command === 10) {
          this.aaPlaybackInferred = 1
          this.mediaStore.patchAaPlayStatus(this.sessions.active(), 1)
        }
        if (msg.command === 11 || msg.command === 2) {
          this.aaPlaybackInferred = 2
          this.mediaStore.patchAaPlayStatus(this.sessions.active(), 2)
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
  }

  private handleCommand(msg: Command): void {
    this.emitProjectionEvent({ type: 'command', message: msg })
    if (typeof msg.value === 'number' && msg.value === 508 && this.anyClusterRequested()) {
      this.driver.requestClusterFocus?.()
    }
  }

  private readonly onDriverMessage = (msg: Message): void => {
    // Always keep updater-relevant state, even if renderer is not attached yet.
    if (msg instanceof SoftwareVersion) return this.handleSoftwareVersion(msg)

    if (msg instanceof BoxInfo) return this.handleBoxInfo(msg)

    if (msg instanceof GnssData) return this.handleGnssData(msg)

    if (!this.webContents) return

    if (msg instanceof BluetoothPairedList) return this.handleBluetoothPairedList(msg)

    if (msg instanceof Plugged) return this.handlePlugged(msg)
    if (msg instanceof BoxUpdateProgress) return this.handleBoxUpdateProgress(msg)
    if (msg instanceof BoxUpdateState) return this.handleBoxUpdateState(msg)
    if (msg instanceof VideoData) return this.handleVideoData(msg)
    if (msg instanceof AudioData) return this.handleAudioData(msg)
    if (msg instanceof Command) return this.handleCommand(msg)
  }

  private onMetaMessage(driver: IPhoneDriver, msg: Message): void {
    const session = this.sessions.byDriver(driver)
    const isActive = session != null && session === this.sessions.active()
    if (msg instanceof MediaData) this.mediaStore.handle(driver, session, msg, isActive)
    else if (msg instanceof NavigationData) this.navStore.handle(driver, session, msg, isActive)
  }

  private readonly onDriverFailure = (): void => {
    const wc = this.webContents
    if (!wc || wc.isDestroyed?.()) return
    wc.send('projection-event', { type: 'failure' })
  }

  private readonly onDriverTargetedConnect = (): void => {
    this.pendingStartupConnectTarget = null
  }

  // 'video-codec' — phone announces which advertised codec it picked
  private readonly onDriverVideoCodec = (codec: 'h264' | 'h265' | 'vp9' | 'av1'): void => {
    this.gstVideoCodec = codec
    const wc = this.webContents
    if (!wc || wc.isDestroyed?.()) return
    wc.send('projection-event', { type: 'video-codec', payload: { codec } })
  }

  private attachCodecCapture(d: IPhoneDriver): void {
    d.on('video-codec', (c: GstVideoCodec) => {
      this.lastMainCodecByDriver.set(d, c)
      const s = this.sessions.byDriver(d)
      if (s) s.video.main.codec = c
      this.sessions.dump(
        `video-codec ${c} → ${s ? `stored on #${s.index}` : 'NO session (map only)'}`
      )
    })
    d.on('cluster-video-codec', (c: GstVideoCodec) => {
      this.lastClusterCodecByDriver.set(d, c)
      const s = this.sessions.byDriver(d)
      if (s) s.video.cluster.codec = c
      this.sessions.dump(
        `cluster-codec ${c} → ${s ? `stored on #${s.index}` : 'NO session (map only)'}`
      )
    })
  }

  private updateVideoCrop(): void {
    const tw = this.lastVideoWidth ?? 0
    const th = this.lastVideoHeight ?? 0
    const dw = this.config.projectionWidth ?? 0
    const dh = this.config.projectionHeight ?? 0
    if (tw > 0 && th > 0 && dw > 0 && dh > 0) {
      const { contentWidth, contentHeight } = aaContentArea(
        { width: tw, height: th },
        { width: dw, height: dh }
      )
      this.videoCrop = {
        cropL: Math.max(0, (tw - contentWidth) / 2),
        cropT: Math.max(0, (th - contentHeight) / 2),
        visW: contentWidth,
        visH: contentHeight,
        tierW: tw,
        tierH: th
      }
    } else {
      this.videoCrop = null
    }
    this.applyVideoCrop()
  }

  private applyVideoCrop(): void {
    const r = this.videoCrop
    this.gstVideo?.setContentRegion(
      r?.cropL ?? 0,
      r?.cropT ?? 0,
      r?.visW ?? 0,
      r?.visH ?? 0,
      r?.tierW ?? 0,
      r?.tierH ?? 0
    )
  }

  private pushGstVideo(nal: Buffer): void {
    const wc = this.webContents
    if (!wc || wc.isDestroyed?.()) return
    if (!this.gstVideo) {
      this.gstVideo = new GstVideo(wc)
      this.gstVideo.setVisible(this.gstVideoVisible)
      this.applyVideoCrop()
      this.emitProjectionEvent({ type: 'projection', shown: true })
    }
    this.gstVideo.push(this.gstVideoCodec, nal)
  }

  private clusterPlaneVisible(screen: ClusterScreen): boolean {
    return screen === 'main' ? this.clusterVisible : true
  }

  private anyClusterRequested(): boolean {
    for (const id of this.clusterRequestedBy) {
      const wc = webContents.fromId(id)
      if (!wc || wc.isDestroyed()) this.clusterRequestedBy.delete(id)
    }
    return this.clusterRequestedBy.size > 0
  }

  private clusterStreamWanted(): boolean {
    return this.anyClusterRequested()
  }

  private syncClusterStreamFocus(): void {
    const want = this.clusterStreamWanted()
    if (want === this.clusterStreamActive) return
    this.clusterStreamActive = want
    this.drivers.setAaClusterStreamActive(want)
    this.drivers.setCpClusterStreamActive(want)
  }

  // The window a cluster plane belongs to: main → main window, dash/aux → ... window
  // mac embeds the video into this window's native view. Linux ignores the handle and
  // places the plane on the target compositor screen instead
  private clusterScreenWebContents(screen: ClusterScreen): WebContents | null {
    if (screen === 'main') return this.webContents ?? null
    const w = getSecondaryWindow(screen)
    return w && !w.isDestroyed() ? w.webContents : null
  }

  private applyClusterCrop(plane: GstVideo): void {
    const tw = this.lastClusterVideoWidth ?? 0
    const th = this.lastClusterVideoHeight ?? 0
    const dw = this.config.clusterWidth ?? 0
    const dh = this.config.clusterHeight ?? 0
    if (tw > 0 && th > 0 && dw > 0 && dh > 0) {
      const { contentWidth, contentHeight } = aaContentArea(
        { width: tw, height: th },
        { width: dw, height: dh }
      )
      plane.setContentRegion(
        Math.max(0, (tw - contentWidth) / 2),
        Math.max(0, (th - contentHeight) / 2),
        contentWidth,
        contentHeight,
        tw,
        th
      )
    } else {
      plane.setContentRegion(0, 0, 0, 0, 0, 0)
    }
  }

  private pushGstVideoCluster(nal: Buffer): void {
    // Focus-stopped: ignore in-flight tail frames. Safe for the decoder because the
    // resume (PROJECTED indication) always restarts the stream at a fresh keyframe.
    if (this.clusterStreamActive === false) return
    // one plane per configured screen, all fed the same cluster stream
    for (const screen of clusterTargetScreens(this.config)) {
      let plane = this.gstVideoClusters.get(screen)
      if (!plane) {
        const wc = this.clusterScreenWebContents(screen)
        if (!wc || wc.isDestroyed?.()) continue
        plane = new GstVideo(wc, `cluster-${screen}`, screen)
        plane.setVisible(this.clusterPlaneVisible(screen))
        this.applyClusterCrop(plane) // fit to the configured cluster-stream AR
        this.gstVideoClusters.set(screen, plane)
      }
      plane.push(this.gstVideoClusterCodec, nal)
    }
  }

  // Renderer reports whether the projection screen is currently shown
  public setVideoVisible(visible: boolean): void {
    this.gstVideoVisible = visible
    this.gstVideo?.setVisible(visible)
  }

  // Cluster plane visibility (cluster:request) drives the main-screen plane only
  public setClusterVisible(visible: boolean): void {
    this.clusterVisible = visible
    this.gstVideoClusters.get('main')?.setVisible(visible)
    this.syncClusterStreamFocus()
  }

  // Cluster channel codec selection
  private readonly onDriverClusterVideoCodec = (codec: 'h264' | 'h265' | 'vp9' | 'av1'): void => {
    this.lastClusterCodec = codec
    this.gstVideoClusterCodec = codec
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

  /** Drive the system-sound blinker click (called from the telemetry store, page/window
   *  independent). */
  public setBlinkerSoundActive(active: boolean): void {
    this.systemSound.setBlinkerActive(active)
  }

  public beginShutdown(): void {
    this.shuttingDown = true
    this.unsubscribeConfigEvents()
    this.systemSound.dispose()
    this.audioMonitor?.stop()
    this.audioMonitor = null
  }

  public async shutdownWirelessSessions(): Promise<void> {
    await this.drivers.releaseAa()
    await this.drivers.releaseCp()
    try {
      await this.aaBtSock.deauthApClients()
    } catch {
      /* best-effort */
    }
    if (this.helperSupervisor) {
      const sup = this.helperSupervisor
      this.helperSupervisor = null
      await sup.stop().catch(() => {})
    }
  }

  constructor() {
    void this.deviceRegistry.load()
    this.drivers = new ProjectionDriverManager({
      handlers: {
        onMessage: (msg) => this.onDriverMessage(msg as Message),
        onMetaMessage: (driver, msg) => this.onMetaMessage(driver, msg),
        onFailure: () => this.onDriverFailure(),
        onTargetedConnect: () => this.onDriverTargetedConnect(),
        onVideoCodec: (c) => this.onDriverVideoCodec(c),
        onClusterVideoCodec: (c) => this.onDriverClusterVideoCodec(c)
      },
      onAaConnected: (s) => this.onAaConnected(s as AaSession),
      onAaDisconnected: (s) => this.onAaDisconnected(s as AaSession),
      onAaPresence: (s, p) => this.onAaPresence(s as AaSession, p),
      onAaCreated: (s) => this.attachCodecCapture(s),
      onAaReleased: () => {},
      getAaConfigSeed: () => ({
        hevcSupported: this.codecCaps.hevc,
        vp9Supported: this.codecCaps.vp9,
        av1Supported: this.codecCaps.av1,
        initialNightMode: deriveInitialNightMode(this.config.appearanceMode)
      }),
      onCpConnected: (s) => this.onCpConnected(s as CpSession),
      onCpDisconnected: (s) => this.onCpDisconnected(s as CpSession),
      onCpPresence: (s, p) => this.onCpPresence(s as CpSession, p),
      onCpHelperPresence: (p) => this.onCpHelperPresence(p),
      onCpHelperConnect: () => this.deviceController.resendReconnectTargets(),
      onCpCreated: (s) => this.attachCodecCapture(s as CpSession),
      onCpReleased: () => {},
      getCpConfigSeed: () => ({
        hevcSupported: this.codecCaps.hevc,
        vp9Supported: this.codecCaps.vp9,
        av1Supported: this.codecCaps.av1,
        initialNightMode: deriveInitialNightMode(this.config.appearanceMode)
      }),
      onPhoneReenumerate: (ms) => this.expectPhoneReenumeration(ms),
      getConfig: () => this.config
    })

    const dongle = this.drivers.getDongle()
    dongle.on('phone-connected', () => this.onDonglePhoneConnected())
    dongle.on('phone-disconnected', () => this.onDonglePhoneDisconnected())
    dongle.on('dongle-info', (info: { boxInfo?: unknown }) => this.onDongleInfo(info))

    this.sessions = new SessionManager({
      route: (d) => this.drivers.route(d),
      onChange: () => {
        this.deviceController.emitDevices()
        this.emitSessionState()
      },
      onActiveChanged: (next, prev) => this.onActiveSessionChanged(next, prev)
    })

    this.deviceRegistry.onChange(() => this.deviceController.emitDevices())

    this.arbiter = new TransportArbiter({
      isWirelessEnabled: () =>
        this.config.wirelessAaEnabled === true && process.platform === 'linux',
      isWirelessPhoneInRange: () => this.wirelessPhoneInRange,
      getActiveTransport: () => this.getActiveTransport(),
      isDongleSessionActive: () => this.getActiveTransport() === 'dongle',
      isWiredAaSessionActive: () => this.started && this.isActiveAaWired(),
      isWiredCpSessionActive: () => this.started && this.isActiveCpWired(),
      hasWiredSession: () =>
        this.started &&
        this.sessions
          .all()
          .some(
            (s) =>
              s.transport === 'usb' && (s.protocol === 'androidauto' || s.protocol === 'carplay')
          ),
      onChange: () => this.emitTransportState(),
      onShouldStop: async () => {
        const a = this.sessions.active()
        if (a) this.sessions.close(a.index)
      },
      onShouldAutoStart: () => {
        this.autoStartIfNeeded().catch(console.error)
      },
      onShouldBringUpWiredBeside: () => {
        this.maybeBringUpWiredBeside().catch(console.error)
      },
      onWiredPhoneGone: () => {
        this.closeWiredPhoneSession()
      }
    })

    this.audio = new ProjectionAudio(
      () => this.config,
      (payload) => {
        this.emitProjectionEvent(payload)
      },
      (channel, data, chunkSize, extra) => {
        // FFT audio chunks must reach every window that can draw the visualizer
        this.sendChunked(channel, data, chunkSize, extra, this.getAllUiWebContents())
      },
      (pcm, decodeType) => {
        this.driver.sendPhoneAudio?.(pcm, decodeType)
      }
    )

    const ipcHost: ProjectionIpcHost = {
      start: () => this.start(),
      stop: () => this.stop(),
      restartSession: () => this.restartSession(),
      setVideoVisible: (v) => this.setVideoVisible(v),
      pickPreferredTransport: () => this.pickPreferredTransport(),
      switchTransport: () => this.switchTransport(),
      getTransportState: () => this.getTransportState(),
      getDevices: () => this.getDevices(),
      selectDevice: (id) => this.selectDevice(id),
      cycleSession: () => this.sessions.activateNext(),
      forgetDevice: (id) => this.forgetDevice(id),
      applyCodecCapabilities: (caps) => this.codecCaps.applyCodecCapabilities(caps),
      send: (msg) => this.driver.send(msg),
      isUsingDongle: () => this.driver instanceof DongleDriver,
      isUsingAa: () => this.getActiveTransport() === 'aa',
      isStarted: () => this.started,
      hasWebUsbDevice: () => this.dongleDriver.isUp,
      sendBluetoothPairedList: (text) => this.dongleDriver.sendBluetoothPairedList(text),
      connectAaBt: (mac) => this.connectPairedDevice(mac),
      removeAaBt: (mac) => this.aaBtSock.remove(mac),
      refreshAaBtPaired: () => {
        this.refreshAaBtPairedList().catch(() => {})
      },
      getBoxInfo: () => this.boxInfo,
      setPendingStartupConnectTarget: (t) => {
        this.pendingStartupConnectTarget = t
      },
      getConfig: () => this.config,
      setClusterRequested: (id, wanted) => {
        if (wanted) this.clusterRequestedBy.add(id)
        else this.clusterRequestedBy.delete(id)
        this.syncClusterStreamFocus()
      },
      isMainClusterWindow: (id) => this.webContents?.id === id,
      isClusterRequested: () => this.anyClusterRequested(),
      setClusterVisible: (v) => this.setClusterVisible(v),
      resetLastClusterVideoSize: () => {
        this.lastClusterVideoWidth = undefined
        this.lastClusterVideoHeight = undefined
      },
      getLastClusterCodec: () => this.lastClusterCodec,
      getLastClusterVideoSize: () => {
        const w = this.lastClusterVideoWidth ?? 0
        const h = this.lastClusterVideoHeight ?? 0
        return w > 0 && h > 0 ? { width: w, height: h } : null
      },
      getClusterTargetWebContents: () => this.getClusterTargetWebContents(),
      uploadIcons: () => this.uploadIcons(),
      getDevToolsUrlCandidates: () => this.getDevToolsUrlCandidates(),
      reloadConfigFromDisk: () => this.reloadConfigFromDisk(),
      getFirmware: () => this.firmware,
      getApkVer: () => this.getApkVer(),
      getDongleFwVersion: () => this.dongleFwVersion,
      emitProjectionEvent: (p) => this.emitProjectionEvent(p),
      readActiveMedia: () => ({
        timestamp: new Date().toISOString(),
        payload: this.sessions.active()?.media ?? DEFAULT_MEDIA_DATA_RESPONSE.payload
      }),
      readActiveNav: () => ({
        timestamp: new Date().toISOString(),
        payload: this.sessions.active()?.nav ?? DEFAULT_NAVIGATION_DATA_RESPONSE.payload
      }),
      setAudioStreamVolume: (s, v) => this.audio.setStreamVolume(s, v),
      setAudioVisualizerEnabled: (e, id) => this.audio.setVisualizerEnabled(e, id)
    }
    registerProjectionIpc(ipcHost)

    this.subscribeConfigEvents()
    this.audioMonitor = startAudioDeviceMonitor(() => {
      this.emitProjectionEvent({ type: 'audioDevicesChanged' })
    })

    this.codecCaps.applyGstCodecCaps()
  }

  private async reloadConfigFromDisk(): Promise<void> {
    try {
      const configPath = path.join(app.getPath('userData'), 'config.json')
      if (!fs.existsSync(configPath)) return
      const userConfig = JSON.parse(fs.readFileSync(configPath, 'utf8')) as Config
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

      let cfg: Config = { ...(DEFAULT_CONFIG as Config), ...this.config }

      try {
        if (fs.existsSync(configPath)) {
          const diskCfg = JSON.parse(fs.readFileSync(configPath, 'utf8')) as Config
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

      this.driver.uploadHostIcons?.(buf120, buf180, buf256)

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

  public applyConfigPatch(patch: Partial<Config>): void {
    this.config = { ...this.config, ...patch }
    this.deviceController.resendReconnectTargets()
    this.syncHelperSupervisor()
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
    if (connected) void this.dongleDriver.bringUp(this.config, this.pendingStartupConnectTarget)
    else void this.dongleDriver.close()
  }

  public markPhoneConnected(connected: boolean, device?: Device): void {
    if (connected) this.startRetryAttempt = 0
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

  public pickPreferredTransport(): Transport | null {
    return this.arbiter.pickPreferred()?.transport ?? null
  }

  public getActiveTransport(): Transport | null {
    const a = this.sessions.active()
    if (a) return a.protocol === 'carplay' ? 'cp' : a.protocol === 'dongle' ? 'dongle' : 'aa'
    return this.started ? 'dongle' : null
  }

  public getTransportState() {
    return this.arbiter.getSnapshot()
  }

  public getDevices(): DeviceView[] {
    return this.deviceController.getDevices()
  }

  public forgetDevice(id: string): { ok: boolean } {
    return this.deviceController.forgetDevice(id)
  }

  public selectDevice(id: string): { ok: boolean } {
    return this.deviceController.selectDevice(id)
  }

  private emitTransportState(): void {
    this.emitProjectionEvent({
      type: 'transportState',
      payload: this.arbiter.getSnapshot()
    })
  }

  public async switchTransport(): Promise<{ ok: boolean; active: Transport | null }> {
    const { ok, target } = this.arbiter.prepareSwitch()
    if (!ok) return { ok: false, active: target?.transport ?? null }

    if (this.isSwitching) {
      return { ok: true, active: target?.transport ?? null }
    }

    this.isSwitching = true
    try {
      while (true) {
        const desired = this.arbiter.getOverride()
        if (!desired) break

        const wasWireless = this.getActiveTransport() === 'aa' && !this.isActiveAaWired()

        if (this.started) {
          try {
            await this.stop()
          } catch (e) {
            console.warn('[ProjectionService] switchTransport: stop threw (ignored)', e)
          }
        }

        if (wasWireless) {
          // Leaving wireless: kick the phone off the AP
          await this.aaBtSock.deauthApClients().catch(() => {})
        }

        if (desired.transport === 'aa' && desired.mode === 'wireless') {
          await this.bounceAaBtConnections()
          // Give BlueZ a moment to commit the disconnect before we re-wake.
          await new Promise((r) => setTimeout(r, 500))
          await this.tryAutoConnect({ force: true })
        }

        await this.autoStartIfNeeded()

        const newOverride = this.arbiter.getOverride()
        if (!newOverride) break
        if (newOverride.transport === desired.transport && newOverride.mode === desired.mode) break
      }
    } finally {
      this.isSwitching = false
    }
    return { ok: true, active: this.getActiveTransport() }
  }

  // Restart the session to apply a config change that needs fresh negotiation
  public async restartSession(): Promise<void> {
    const aaRouted = this.getActiveTransport() === 'aa'
    const wasWired = aaRouted && this.isActiveAaWired()
    const wasWireless = aaRouted && !this.isActiveAaWired()

    try {
      await this.stop()
    } catch (e) {
      console.warn('[ProjectionService] restartSession: stop threw (ignored)', e)
    }

    if (wasWired) {
      return
    }

    if (wasWireless) {
      await this.bounceAaBtConnections()
      await new Promise((r) => setTimeout(r, 500))
      await this.tryAutoConnect({ force: true })
    }

    await this.autoStartIfNeeded()
  }

  // Device-list connect entry: phone → switch to wireless AA targeting this MAC
  public async connectPairedDevice(mac: string): Promise<{ ok: boolean; error?: string }> {
    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch (e) {
      return { ok: false, error: (e as Error).message }
    }
    const upper = mac.toUpperCase()
    const dev = devices.find((d) => d.mac.toUpperCase() === upper)

    if (!dev || !isPhoneLikeCod(dev.class)) {
      return await this.aaBtSock.connectFull(mac)
    }

    if (this.isSwitching) return { ok: false, error: 'switch in progress' }
    this.isSwitching = true
    try {
      const wasWireless = this.getActiveTransport() === 'aa' && !this.isActiveAaWired()

      if (this.started) {
        try {
          await this.stop()
        } catch (e) {
          console.warn('[ProjectionService] connectPairedDevice: stop threw (ignored)', e)
        }
      }
      if (wasWireless) {
        await this.aaBtSock.deauthApClients().catch(() => {})
      }

      this.applyConfigPatch({ ...this.config, lastConnectedAaBtMac: mac })
      this.arbiter.setOverride({ transport: 'aa', mode: 'wireless' })

      await this.bounceAaBtConnections()
      await new Promise((r) => setTimeout(r, 500))
      await this.tryAutoConnect({ force: true })
      await this.autoStartIfNeeded()

      return { ok: true }
    } finally {
      this.isSwitching = false
    }
  }

  public async disconnectHostBtPhones(): Promise<void> {
    if (process.platform !== 'linux') return
    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch {
      return
    }
    for (const d of devices) {
      if (!d.connected) continue
      if (!isPhoneLikeCod(d.class)) continue
      try {
        console.log(`[ProjectionService] shutdown disconnect ${d.mac}`)
        await this.aaBtSock.disconnect(d.mac)
      } catch (e) {
        console.warn('[ProjectionService] shutdown BT disconnect threw', e)
      }
    }
  }

  private async bounceAaBtConnections(): Promise<void> {
    if (process.platform !== 'linux') return
    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch {
      return
    }
    for (const d of devices) {
      if (!d.connected) continue
      // Only bounce phones; audio devices keep their A2DP link
      if (!isPhoneLikeCod(d.class)) continue
      try {
        console.log(`[ProjectionService] bounce BT ${d.mac} to retrigger wireless AA`)
        await this.aaBtSock.disconnect(d.mac)
      } catch (e) {
        console.warn('[ProjectionService] BT disconnect during bounce threw', e)
      }
    }
  }

  /** BT MACs held by a CarPlay session, so the AA name correlation skips them. */
  private cpClaimedBtMacs(): Set<string> {
    return new Set(
      this.sessions
        .all()
        .filter((s) => s.protocol === 'carplay' && s.device.btMac)
        .map((s) => (s.device.btMac as string).toUpperCase())
    )
  }

  private async refreshAaBtPairedList(
    opts: { throwOnError?: boolean; preferMac?: string } = {}
  ): Promise<void> {
    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch (e) {
      if (opts.throwOnError) throw e
      return
    }

    const phones = devices.filter((d) => isPhoneLikeCod(d.class))
    const cpBtMacs = this.cpClaimedBtMacs()
    const connected =
      phones.find((d) => d.connected && !cpBtMacs.has(d.mac.toUpperCase()))?.mac ?? ''
    this.aaBtNameByMac = new Map(devices.map((d) => [d.mac.toUpperCase(), d.name || '']))
    for (const p of phones) if (p.name) this.deviceRegistry.noteName(p.mac, p.name)
    const preferUp = Boolean(
      opts.preferMac &&
        phones.some((d) => d.connected && d.mac.toUpperCase() === opts.preferMac?.toUpperCase())
    )
    if (preferUp && opts.preferMac) this.connectedAaBtMac = opts.preferMac
    else if (connected) this.connectedAaBtMac = connected
    const wasSettled = this.btInitialQueryDone
    this.btInitialQueryDone = true
    // Wired AA doesn't wake the phone over BT — treat any paired phone as in-range
    const wiredAaActive = this.started && this.isActiveAaWired()
    const offerable = connected !== '' || (wiredAaActive && phones.length > 0)
    this.setWirelessPhoneInRange(offerable)
    if (!wasSettled) this.autoStartIfNeeded().catch(console.error)

    // Ignore transient empty responses to avoid UI flicker
    if (devices.length === 0 && this.hostDevList.length > 0) {
      console.warn('[ProjectionService] empty paired list, keeping last known host entries')
    } else {
      this.hostDevList = devices.map((d) => ({
        id: d.mac,
        name: d.name || d.mac,
        type: isPhoneLikeCod(d.class) ? 'AndroidAuto' : '',
        source: 'host',
        class: d.class,
        connected: d.connected
      }))
      this.hostPairedRaw = devices.length
        ? devices.map((d) => `${d.mac}${d.name ?? ''}`).join('\n') + '\n'
        : ''
    }

    if (this.aaBtActive && connected && this.config.lastConnectedAaBtMac !== connected) {
      configEvents.emit('requestSave', { lastConnectedAaBtMac: connected })
    }
    this.emitCombinedBtPairedList()
    this.deviceController.emitDevices()
  }

  private async populateAaBtPairedListInitial(): Promise<void> {
    const totalTimeoutMs = 30_000
    const intervalMs = 2_000
    const deadline = Date.now() + totalTimeoutMs
    const expectDevice = !!this.config.lastConnectedAaBtMac

    while (Date.now() < deadline) {
      if (!this.aaBtActive) return
      try {
        const devices = await this.aaBtSock.listPaired()
        await this.refreshAaBtPairedList().catch(() => {})
        if (devices.length === 0 && expectDevice) {
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

  private extractBluezMac(deviceName: string | undefined | null): string | null {
    if (!deviceName) return null
    // bluez_output uses underscores, bluez_input uses colons
    const m = deviceName.match(/^bluez_(?:output|input|sink|source)\.([0-9A-Fa-f_:]{17})/)
    return m ? m[1]!.replace(/_/g, ':').toUpperCase() : null
  }

  // Host wins on MAC collision so a natively paired phone keeps no (D) suffix
  private mergedDevList(): DevListEntry[] {
    const norm = (id: string | undefined): string => (id ?? '').toUpperCase()
    const hostMacs = new Set(this.hostDevList.map((e) => norm(e.id)))
    const dongleUnique = this.dongleDevList.filter((e) => !hostMacs.has(norm(e.id)))
    return [...this.hostDevList, ...dongleUnique]
  }

  private emitCombinedBtPairedList(): void {
    if (!this.webContents) return
    const parse = (raw: string): Array<{ mac: string; line: string }> => {
      const out: Array<{ mac: string; line: string }> = []
      for (const line of raw.split('\n')) {
        const trimmed = line.replace(/\r$/, '').replace(/\0+$/g, '')
        if (trimmed.length < 17) continue
        const mac = trimmed.slice(0, 17).toUpperCase()
        if (!mac.includes(':')) continue
        out.push({ mac, line: trimmed })
      }
      return out
    }
    const dongle = parse(this.donglePairedRaw)
    const dongleMacs = new Set(dongle.map((d) => d.mac))
    const host = parse(this.hostPairedRaw).filter((h) => !dongleMacs.has(h.mac))
    const all = [...dongle, ...host]
    const raw = all.length ? all.map((d) => d.line).join('\n') + '\n' : ''
    this.emitProjectionEvent({ type: 'bluetoothPairedList', payload: raw })
  }

  private async connectConfiguredAudioDevices(): Promise<void> {
    if (!this.aaBtActive) return
    const macs = new Set<string>()
    const outMac = this.extractBluezMac(this.config.audioOutputDevice)
    const inMac = this.extractBluezMac(this.config.audioInputDevice)
    if (outMac) macs.add(outMac)
    if (inMac) macs.add(inMac)
    if (macs.size === 0) return

    let paired
    try {
      paired = await this.aaBtSock.listPaired()
    } catch {
      return
    }
    for (const mac of macs) {
      const dev = paired.find((d) => d.mac.toUpperCase() === mac)
      if (!dev) {
        console.log(`[ProjectionService] audio device ${mac} not paired, skipping autoconnect`)
        continue
      }
      if (dev.connected) {
        console.log(`[ProjectionService] audio device ${mac} already connected`)
        continue
      }
      // Device1.Connect (all profiles) with retry — device may not be ready yet
      const maxAttempts = 4
      const retryDelayMs = 4000
      for (let attempt = 1; attempt <= maxAttempts; attempt++) {
        console.log(
          `[ProjectionService] connecting audio device ${mac} (A2DP + HFP) attempt ${attempt}/${maxAttempts}`
        )
        let resp: { ok: boolean; error?: string }
        try {
          resp = await this.aaBtSock.connectFull(mac)
        } catch (e) {
          console.warn(`[ProjectionService] audio device ${mac} connect threw`, e)
          break
        }
        if (resp.ok) {
          console.log(`[ProjectionService] audio device ${mac} connected`)
          break
        }
        console.warn(
          `[ProjectionService] audio device ${mac} connect failed (attempt ${attempt}): ${resp.error}`
        )
        if (attempt < maxAttempts) {
          await new Promise((r) => setTimeout(r, retryDelayMs))
        }
      }
    }
  }

  // Pick a target from the paired list and fire a single Connect
  private async tryAutoConnect(opts: { force?: boolean } = {}): Promise<void> {
    if (!this.aaBtActive) {
      console.log('[ProjectionService] autoconnect: skipped (wireless AA not active)')
      return
    }
    // Don't poke the phone over BT while a wired session is already running
    if (this.started && this.isActiveAaWired()) {
      console.log('[ProjectionService] autoconnect: skipped (wired AA session active)')
      return
    }
    // Passive autostart: skip if wired phone present. Manual switch sets force.
    if (!opts.force && this.arbiter.getSnapshot().wiredPhoneDetected) {
      console.log('[ProjectionService] autoconnect: skipped (wired phone detected)')
      return
    }

    let devices
    try {
      devices = await this.aaBtSock.listPaired()
    } catch {
      return
    }
    // Audio devices being connected doesn't count — we still want to wake the phone
    const phones = devices.filter((d) => isPhoneLikeCod(d.class))
    const connected = phones.filter((d) => d.connected)
    if (connected.length > 0) {
      console.log(
        `[ProjectionService] autoconnect: skipped (already connected: ${connected.map((d) => d.mac).join(', ')})`
      )
      return
    }

    const lastMac = this.config.lastConnectedAaBtMac
    const preferred = lastMac ? phones.find((d) => d.mac === lastMac) : null
    const trusted = phones.filter((d) => d.trusted)
    const target = preferred || trusted[0] || phones[0]
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

  private dispatchRemoteInput(command: string): void {
    if (!isInputCommand(command)) {
      console.warn(`[ProjectionService] remote input: unknown command "${command}"`)
      return
    }
    if (!this.started) return
    try {
      this.driver.handleInput(command)
    } catch (e) {
      console.warn(`[ProjectionService] remote input "${command}" failed`, e)
    }
  }

  // Open the long-lived aa-bt event subscription
  private openAaBtSubscription(): void {
    if (this.aaBtSubscription) return
    const open = (): void => {
      if (!this.aaBtActive) return
      this.aaBtSubscription = this.aaBtSock.subscribe(
        (ev) => {
          if (ev.event === 'input' && ev.command) {
            this.dispatchRemoteInput(ev.command)
            return
          }
          if (ev.event === 'aa-device') {
            if (typeof ev.btMac === 'string' && typeof ev.instanceId === 'string') {
              this.aaBtMacByInstance.set(ev.instanceId, ev.btMac)
            }
            if (typeof ev.usbSerial === 'string' && ev.usbSerial && ev.instanceId) {
              this.aaSerialByInstance.set(ev.instanceId, ev.usbSerial)
            }
            return
          }
          this.refreshAaBtPairedList({
            preferMac: typeof ev.mac === 'string' ? ev.mac : undefined
          }).catch(() => {})
        },
        () => {
          this.aaBtSubscription = null
          if (this.aaBtActive) setTimeout(open, 1000)
        },
        () => this.deviceController.resendReconnectTargets()
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

  private async armWiredAa(device: Device): Promise<boolean> {
    return this.drivers.bringUpAaWired(device)
  }

  private async maybeBringUpWiredBeside(): Promise<void> {
    const device = this.arbiter.getPhoneDevice()
    if (!device) return
    if (device.vendorId === APPLE_VENDOR_ID) return
    const aaSessions = this.sessions.all().filter((s) => s.protocol === 'androidauto')
    // A live WIRED AA session = a 2nd Android already streaming (Tier B) → skip.
    if (aaSessions.some((s) => s.transport === 'usb')) return
    // The wireless session stays up until the wired one has identified. The
    // SessionManager then hands the entry to the wired driver and retires the
    // wireless one, so the phone never tears down and re-enumerates at once.
    console.log('[ProjectionService] wired AA bring-up beside active session')
    try {
      await this.armWiredAa(device)
    } catch (e) {
      console.warn('[ProjectionService] wired-beside AA bring-up failed', e)
    }
  }

  private closeWiredPhoneSession(): void {
    const wired = this.sessions
      .all()
      .find((s) => s.protocol === 'androidauto' && s.transport === 'usb')
    if (!wired) return
    // Closing the wired AaSession tears down its bridge. The AaManager keeps the
    // :5277 wireless listener up, so the phone can come back over WiFi on its own.
    void (wired.driver as AaSession).close()
  }

  public async autoStartIfNeeded() {
    if (this.shuttingDown) return
    if (this.isStopping && this.stopPromise) {
      try {
        await this.stopPromise
      } catch {}
    }
    if (this.shuttingDown) return
    if (this.sessions.all().length > 0) return
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
        const candidate = this.arbiter.pickPreferred()
        const target: Transport =
          candidate?.transport === 'aa' ? 'aa' : candidate?.transport === 'cp' ? 'cp' : 'dongle'
        // Dongle is brought up on USB attach (bringUpDongle), never through start().
        if (target === 'dongle') return

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

        this.mediaStore.reset('session-start')
        this.navStore.reset('session-start')

        const useAa = target === 'aa'

        if (target === 'cp') {
          // The CarPlay :7000 listener + helper feed are owned by CpManager. Ensure
          // they are up; a CpSession spawns and auto-activates when the phone connects.
          this.drivers.startCp()
          this.started = true
          this.clearStartRetry()
          console.log(
            `[ProjectionService] started in CP mode (${candidate?.mode === 'wired' ? 'wired' : 'wireless'})`
          )
          this.clusterStreamActive = null
          this.syncClusterStreamFocus()
          return
        }

        if (useAa) {
          // Two AA paths: Wired (per-device AOAP bring-up) + Wireless (:5277 listener)
          const wantWired = candidate?.mode === 'wired'
          const wiredDevice = wantWired ? this.arbiter.getPhoneDevice() : null

          if (wantWired && !wiredDevice) {
            console.warn('[ProjectionService] wired phone has no live handle yet — retrying')
            this.started = false
            this.scheduleStartRetry()
            return
          }

          if (wiredDevice) {
            console.log(
              `[ProjectionService] wired AA bring-up with device vid=0x${wiredDevice.vendorId.toString(16)} pid=0x${wiredDevice.productId.toString(16)}`
            )
            try {
              const ok = await this.drivers.bringUpAaWired(wiredDevice)
              this.started = ok
              if (this.started) {
                this.clearStartRetry()
                console.log('[ProjectionService] started in AA mode (wired)')
                // Fresh AAStack defaults to an active cluster stream, re-apply visibility state
                this.clusterStreamActive = null
                this.syncClusterStreamFocus()
              } else {
                console.warn(
                  '[ProjectionService] wired AA bring-up returned false — session not running, retrying'
                )
                this.scheduleStartRetry()
              }
            } catch (e) {
              console.warn('[ProjectionService] AA wired start failed, retrying', e)
              this.started = false
              this.scheduleStartRetry()
            }
          } else {
            // The AaManager's :5277 wireless listener is already armed by
            // syncHelperSupervisor; ensure it is up and mark the session running.
            console.log('[ProjectionService] wireless AA bring-up (listener already armed)')
            this.drivers.startAaWireless()
            this.started = true
            this.clearStartRetry()
            // Fresh AAStack defaults to an active cluster stream, re-apply visibility state
            this.clusterStreamActive = null
            this.syncClusterStreamFocus()
          }
          return
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
    return (await this.driver.disconnectPhone?.()) ?? false
  }

  private lastSessionKey = ''

  private emitSessionState(): void {
    const ordered = this.sessions.all().sort((a, b) => a.index - b.index)
    const active = this.sessions.active()
    const protocol = active?.protocol ?? null
    const position = active ? ordered.findIndex((s) => s === active) + 1 : 0
    const key = `${protocol}:${position}:${ordered.length}`
    if (key === this.lastSessionKey) return
    this.lastSessionKey = key
    this.emitProjectionEvent({ type: 'session', protocol, position, total: ordered.length })
  }

  private onActiveSessionChanged(
    next: ProjectionSession | null,
    prev: ProjectionSession | null
  ): void {
    this.emitSessionState()
    if (next) {
      console.log(`[ProjectionService] active session -> #${next.index} ${next.protocol}`)
      if (next.protocol === 'dongle') {
        this.started = true
        if (prev) {
          this.disposeGstPlanes()
          if (!this.isStarting) next.driver.requestKeyframe?.()
        }
        if (!prev) this.audio.resetForSessionStart()
        this.mediaStore.hydrate(next)
        this.navStore.hydrate(next)
        return
      }
      this.disposeGstPlanes()
      this.mediaStore.hydrate(next)
      this.navStore.hydrate(next)
      const mc = next.video.main.codec ?? this.lastMainCodecByDriver.get(next.driver)
      const cc = next.video.cluster.codec ?? this.lastClusterCodecByDriver.get(next.driver)
      if (mc) this.gstVideoCodec = mc
      if (cc) this.gstVideoClusterCodec = cc
      console.log(
        `[SESSIONS] codec-restore #${next.index} ${next.protocol}: session=${next.video.main.codec ?? '-'} map=${this.lastMainCodecByDriver.get(next.driver) ?? '-'} → gstVideoCodec=${this.gstVideoCodec}`
      )
      this.lastVideoWidth = next.video.main.width
      this.lastVideoHeight = next.video.main.height
      this.lastClusterVideoWidth = next.video.cluster.width
      this.lastClusterVideoHeight = next.video.cluster.height
      this.updateVideoCrop()
      if (!this.isStarting) {
        if (!prev) this.audio.resetForSessionStart()
        next.driver.requestKeyframe?.()
      }
    } else {
      this.teardownToIdle()
    }
  }

  private teardownToIdle(): void {
    if (this.stopping || this.isStopping || this.shuttingDown) return
    this.disposeGstPlanes()
    this.emitProjectionEvent({ type: 'projection', shown: false })
    this.audio.resetForSessionStop()
    this.started = false
    this.statusFile.setStreaming(false)
    this.mediaStore.reset('session-idle')
    this.navStore.reset('session-idle')
    const wc = this.webContents
    if (wc && !wc.isDestroyed()) {
      try {
        wc.send('projection-event', { type: 'unplugged' })
      } catch {}
    }
    this.emitProjectionEvent({ type: 'unplugged' })
    this.autoStartIfNeeded().catch(() => {})
  }

  public async stop(): Promise<void> {
    if (this.isStopping) return this.stopPromise ?? Promise.resolve()
    if (!this.started || this.stopping) return

    this.stopping = true
    this.isStopping = true
    this.sessions.clear()
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

      const wasDongleSession = this.driver instanceof DongleDriver

      if (wasDongleSession) {
        try {
          await this.driver.close()
        } catch (e) {
          console.warn('[ProjectionService] dongle close() failed (ignored)', e)
        }
      }

      if (wasDongleSession) {
        // Dongle gone — drop its stale DevList
        this.dongleDevList = []
        this.donglePairedRaw = ''
        this.dongleConnectedMac = ''
      }

      this.audio.resetForSessionStop()

      this.disposeGstPlanes()

      this.started = false
      this.mediaStore.reset('session-stop')
      this.navStore.reset('session-stop')

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

  // Bring-up can fail transiently (USB interface still busy, phone still locked). Keep retrying
  // so a connection eventually establishes, the arbiter stops us once the phone is gone.
  private scheduleStartRetry() {
    if (this.shuttingDown || this.stopping) return
    if (this.startRetryTimer) return
    const delay = Math.min(START_RETRY_CAP_MS, START_RETRY_BASE_MS * 2 ** this.startRetryAttempt)
    this.startRetryAttempt++
    this.startRetryTimer = setTimeout(() => {
      this.startRetryTimer = null
      this.autoStartIfNeeded().catch(console.error)
    }, delay)
    this.startRetryTimer.unref?.()
  }

  private clearStartRetry() {
    this.startRetryAttempt = 0
    if (this.startRetryTimer) {
      clearTimeout(this.startRetryTimer)
      this.startRetryTimer = null
    }
  }

  private clearTimeouts() {
    this.clearStartRetry()
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
  // video chunks + resolution events, derived from the cluster dashboards
  // (dash3/dash4) per screen. Falls back to the bound main webContents when
  // settings are missing so the path stays compatible with tests / startup.
  private getClusterTargetWebContents(): WebContents[] {
    const screens = clusterTargetScreens(this.config)
    const isAlive = (wc: WebContents | null | undefined): wc is WebContents => {
      if (!wc) return false
      try {
        return typeof wc.isDestroyed !== 'function' || !wc.isDestroyed()
      } catch {
        return true
      }
    }
    const out: WebContents[] = []
    if (screens.includes('main') && isAlive(this.webContents)) {
      out.push(this.webContents as WebContents)
    }
    if (screens.includes('dash')) {
      const w = getSecondaryWindow('dash')
      if (w && !w.isDestroyed() && isAlive(w.webContents)) out.push(w.webContents)
    }
    if (screens.includes('aux')) {
      const w = getSecondaryWindow('aux')
      if (w && !w.isDestroyed() && isAlive(w.webContents)) out.push(w.webContents)
    }
    if (out.length === 0 && isAlive(this.webContents)) {
      out.push(this.webContents as WebContents)
    }
    return out
  }

  // Every live UI window (main + secondary). Used for data every window may render,
  // e.g. the FFT audio chunks, which otherwise only reach the main window.
  private getAllUiWebContents(): WebContents[] {
    const alive = (wc: WebContents | null | undefined): wc is WebContents => {
      try {
        return !!wc && (typeof wc.isDestroyed !== 'function' || !wc.isDestroyed())
      } catch {
        return !!wc
      }
    }
    const out: WebContents[] = []
    if (alive(this.webContents)) out.push(this.webContents as WebContents)
    for (const role of ['dash', 'aux'] as const) {
      const w = getSecondaryWindow(role)
      if (w && !w.isDestroyed() && alive(w.webContents)) out.push(w.webContents)
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
