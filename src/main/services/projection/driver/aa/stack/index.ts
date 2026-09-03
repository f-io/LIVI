/**
 * AA stack, Wireless Android Auto protocol engine for LIVI.
 *
 * Public API:
 *
 *   const aa = new AAStack({ huName: 'LIVI' })
 *
 *   aa.on('session',      (session) => { ... })   // new phone connected
 *   aa.on('video-codec',  (codec) => { ... })     // 'h264' | 'h265' chosen by phone at START_INDICATION
 *   aa.on('error',        (err) => { ... })
 *
 *   aa.stop()                           // closes the active session
 *   aa.sendTouch(action, pointers)      // forward touch event to phone
 *   aa.sendButton(keyCode, down)        // forward HW button event to phone
 *
 */

import { EventEmitter } from 'node:events'
import type { AudioChannelType } from './channels/AudioChannel'
import type { TouchPointer } from './channels/InputChannel'
import type { MediaPlaybackMetadata, MediaPlaybackStatus } from './channels/MediaInfoChannel'
import type {
  NavigationDistanceUpdate,
  NavigationPositionUpdate,
  NavigationStateUpdate,
  NavigationStatusUpdate,
  NavigationTurnUpdate
} from './channels/NavigationChannel'
import { Session, type SessionConfig, type VideoCodec } from './session/Session'
import { detectBtMac, detectWifiBssid } from './system/hwaddr'
import type { HelperSessionLink } from './transport/HelperSessionLink'

export type { AudioChannelType } from './channels/AudioChannel.js'
export { BUTTON_KEY, TOUCH_ACTION, type TouchPointer } from './channels/InputChannel.js'
export type {
  MediaPlaybackMetadata,
  MediaPlaybackState,
  MediaPlaybackStatus
} from './channels/MediaInfoChannel.js'
export type {
  NavigationDistanceUpdate,
  NavigationPositionUpdate,
  NavigationState,
  NavigationStateUpdate,
  NavigationStatusUpdate,
  NavigationTurnEvent,
  NavigationTurnSide,
  NavigationTurnUpdate
} from './channels/NavigationChannel.js'
export type { SessionConfig, VideoCodec } from './session/Session'
export { Session } from './session/Session.js'
export { detectBtMac, detectWifiBssid } from './system/hwaddr'

export type AAStackConfig = SessionConfig

export class AAStack extends EventEmitter {
  private _activeSession: Session | null = null
  private _clusterStreamActive = true
  private _configRefresh: (() => void) | null = null

  constructor(private readonly _cfg: AAStackConfig) {
    super()
    _cfg.btMacAddress ??= detectBtMac()
    _cfg.wifiBssid ??= detectWifiBssid()
  }

  private _adoptSession(session: Session): void {
    this._activeSession = session
    session.setClusterStreamActive(this._clusterStreamActive)

    session.on('video-codec', (codec: VideoCodec) => this.emit('video-codec', codec))
    session.on('cluster-video-codec', (codec: VideoCodec) =>
      this.emit('cluster-video-codec', codec)
    )
    session.on('audio-setup', (channel: AudioChannelType, sampleRate: number, channels: number) =>
      this.emit('audio-setup', channel, sampleRate, channels)
    )
    session.on('audio-start', (channel: AudioChannelType, channelId: number) =>
      this.emit('audio-start', channel, channelId)
    )
    session.on('audio-stop', (channel: AudioChannelType, channelId: number) =>
      this.emit('audio-stop', channel, channelId)
    )
    session.on('mic-start', (channelId: number) => this.emit('mic-start', channelId))
    session.on('mic-stop', (channelId: number) => this.emit('mic-stop', channelId))
    session.on('voice-session', (active: boolean) => this.emit('voice-session', active))
    session.on('audio-focus', (focusType: number) => this.emit('audio-focus', focusType))
    session.on('host-ui-requested', () => this.emit('host-ui-requested'))
    session.on(
      'device-info',
      (d: { name: string; model: string; instanceId: string; ip: string }) =>
        this.emit('device-info', d)
    )
    session.on('device-status', (s: Record<string, unknown>) => this.emit('device-status', s))
    session.on('video-focus-projected', () => this.emit('video-focus-projected'))
    session.on('cluster-video-focus-projected', () => this.emit('cluster-video-focus-projected'))
    session.on('video-started', () => this.emit('video-started'))
    session.on('cluster-video-started', () => this.emit('cluster-video-started'))
    session.on('media-metadata', (m: MediaPlaybackMetadata) => this.emit('media-metadata', m))
    session.on('media-status', (s: MediaPlaybackStatus) => this.emit('media-status', s))
    session.on('nav-start', () => this.emit('nav-start'))
    session.on('nav-stop', () => this.emit('nav-stop'))
    session.on('nav-status', (s: NavigationStatusUpdate) => this.emit('nav-status', s))
    session.on('nav-turn', (t: NavigationTurnUpdate) => this.emit('nav-turn', t))
    session.on('nav-distance', (d: NavigationDistanceUpdate) => this.emit('nav-distance', d))
    session.on('nav-state', (s: NavigationStateUpdate) => this.emit('nav-state', s))
    session.on('nav-position', (p: NavigationPositionUpdate) => this.emit('nav-position', p))
    session.on('connected', () => this.emit('connected'))
    session.on('disconnected', (reason?: string) => this.emit('disconnected', reason))
    session.on('error', (err: Error) => this.emit('error', err))

    this.emit('session', session)
  }

  applyDisplayConfig(next: AAStackConfig): void {
    Object.assign(this._cfg, next)
  }

  setConfigRefresh(fn: () => void): void {
    this._configRefresh = fn
  }

  /** A session the helper carries: it did the transport, version and TLS already. */
  attachLink(link: HelperSessionLink): Session {
    const label = `helper ${link.peer}`
    this._configRefresh?.()
    const session = new Session(link, this._cfg)
    session.on('error', (err: Error) => console.error(`[Session ${label}] error:`, err.message))
    session.on('disconnected', (reason?: string) =>
      console.log(`[Session ${label}] disconnected: ${reason ?? ''}`)
    )
    this._adoptSession(session)
    void session.start().catch((err: Error) => {
      console.error(`[Session ${label}] start error:`, err.message)
    })
    return session
  }

  sendMediaSink(cfg: Record<string, unknown>): void {
    this._activeSession?.sendMediaSink(cfg)
  }

  stop(): void {
    if (this._activeSession) {
      try {
        this._activeSession.close('stack restart')
      } catch (e) {
        console.warn('[AAStack] active session close threw (ignored)', e)
      }
      this._activeSession = null
    }
  }

  get activeSession(): Session | null {
    return this._activeSession
  }

  sendTouch(action: number, pointers: TouchPointer[], actionIndex = 0): void {
    this._activeSession?.sendTouch(action, pointers, actionIndex)
  }

  sendButton(keyCode: number | readonly number[], down: boolean): void {
    this._activeSession?.sendButton(keyCode, down)
  }

  sendRotary(direction: -1 | 1): void {
    this._activeSession?.sendRotary(direction)
  }

  sendFuelData(level: number, range?: number, lowFuelWarning?: boolean): void {
    this._activeSession?.sendFuelData(level, range, lowFuelWarning)
  }

  sendSpeedData(speedMmS: number, cruiseEngaged?: boolean, cruiseSetSpeedMmS?: number): void {
    this._activeSession?.sendSpeedData(speedMmS, cruiseEngaged, cruiseSetSpeedMmS)
  }

  sendRpmData(rpmE3: number): void {
    this._activeSession?.sendRpmData(rpmE3)
  }

  sendGearData(gear: number): void {
    this._activeSession?.sendGearData(gear)
  }

  sendNightModeData(nightMode: boolean): void {
    this._activeSession?.sendNightModeData(nightMode)
  }

  sendParkingBrakeData(engaged: boolean): void {
    this._activeSession?.sendParkingBrakeData(engaged)
  }

  sendLightData(headLight?: 1 | 2 | 3, hazardLights?: boolean, turnIndicator?: 1 | 2 | 3): void {
    this._activeSession?.sendLightData(headLight, hazardLights, turnIndicator)
  }

  sendEnvironmentData(temperatureE3?: number, pressureE3?: number, rain?: number): void {
    this._activeSession?.sendEnvironmentData(temperatureE3, pressureE3, rain)
  }

  sendOdometerData(totalKmE1: number, tripKmE1?: number): void {
    this._activeSession?.sendOdometerData(totalKmE1, tripKmE1)
  }

  sendDrivingStatusData(status: number): void {
    this._activeSession?.sendDrivingStatusData(status)
  }

  sendGpsLocationData(opts: {
    latDeg: number
    lngDeg: number
    accuracyM?: number
    altitudeM?: number
    speedMs?: number
    bearingDeg?: number
  }): void {
    this._activeSession?.sendGpsLocationData(opts)
  }

  sendVehicleEnergyModel(
    capacityWh: number,
    currentWh: number,
    rangeM: number,
    opts?: { maxChargePowerW?: number; maxDischargePowerW?: number; auxiliaryWhPerKm?: number }
  ): void {
    this._activeSession?.sendVehicleEnergyModel(capacityWh, currentWh, rangeM, opts)
  }

  /** Where the pipeline's microphone tap has to deliver, known once the session is ready. */
  micSocketPath(): string | null {
    return this._activeSession?.micSocketPath() ?? null
  }

  requestVideoFocus(): void {
    this._activeSession?.requestVideoFocus()
  }

  requestMainKeyframe(): void {
    this._activeSession?.requestMainKeyframe()
  }

  /** The mic capture format the phone negotiated, 16 kHz mono until setup arrives. */
  micFormat(): { sampleRate: number; channels: number } {
    return this._activeSession?.micFormat() ?? { sampleRate: 16000, channels: 1 }
  }

  requestClusterKeyframe(): void {
    this._activeSession?.requestClusterKeyframe()
  }

  forceClusterKeyframe(): void {
    this._activeSession?.forceClusterKeyframe()
  }

  setClusterStreamActive(active: boolean): void {
    this._clusterStreamActive = active
    this._activeSession?.setClusterStreamActive(active)
  }

  async requestShutdown(): Promise<void> {
    await this._activeSession?.requestShutdown()
  }
}
