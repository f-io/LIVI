/**
 * AA stack — Wireless Android Auto protocol engine for LIVI.
 *
 * Public API:
 *
 *   const aa = new AAStack({ huName: 'LIVI' })
 *
 *   aa.on('session',      (session) => { ... })   // new phone connected
 *   aa.on('video-frame',  (buf, ts) => { ... })   // H.264 NAL unit from first session
 *   aa.on('audio-frame',  (buf, ts, ch, chId) => { ... })   // PCM samples
 *   aa.on('error',        (err) => { ... })
 *
 *   aa.start()                          // begins listening on TCP port 5277
 *   aa.stop()                           // closes the server
 *   aa.sendTouch(action, pointers)      // forward touch event to phone
 *   aa.sendButton(keyCode, down)        // forward HW button event to phone
 *
 */

import { EventEmitter } from 'node:events'
import type { AudioChannelType } from './channels/AudioChannel'
import type { TouchPointer } from './channels/InputChannel'
import type { MediaPlaybackMetadata, MediaPlaybackStatus } from './channels/MediaInfoChannel'
import { Session, type SessionConfig } from './session/Session'
import { detectBtMac, detectWifiBssid } from './system/hwaddr'
import { TcpServer } from './transport/TcpServer'

export type { AudioChannelType } from './channels/AudioChannel.js'
export { BUTTON_KEY, TOUCH_ACTION, type TouchPointer } from './channels/InputChannel.js'
export type {
  MediaPlaybackMetadata,
  MediaPlaybackState,
  MediaPlaybackStatus
} from './channels/MediaInfoChannel.js'
export { TCP_PORT } from './constants'
export type { SessionConfig } from './session/Session'
export { Session } from './session/Session.js'
export { detectBtMac, detectWifiBssid } from './system/hwaddr'
export { TcpServer } from './transport/TcpServer'

export interface AAStackConfig extends SessionConfig {
  port?: number
}

export class AAStack extends EventEmitter {
  private readonly _server: TcpServer
  private _activeSession: Session | null = null

  constructor(private readonly _cfg: AAStackConfig) {
    super()
    const resolvedCfg: AAStackConfig = {
      ..._cfg,
      btMacAddress: _cfg.btMacAddress ?? detectBtMac(),
      wifiBssid: _cfg.wifiBssid ?? detectWifiBssid()
    }
    this._server = new TcpServer(resolvedCfg)

    this._server.on('session', (session: Session) => {
      this._activeSession = session

      session.on('video-frame', (buf: Buffer, ts: bigint) => this.emit('video-frame', buf, ts))
      session.on(
        'audio-frame',
        (buf: Buffer, ts: bigint, channel: AudioChannelType, channelId: number) =>
          this.emit('audio-frame', buf, ts, channel, channelId)
      )
      session.on('audio-start', (channel: AudioChannelType, channelId: number) =>
        this.emit('audio-start', channel, channelId)
      )
      session.on('audio-stop', (channel: AudioChannelType, channelId: number) =>
        this.emit('audio-stop', channel, channelId)
      )
      session.on('mic-start', (channelId: number) => this.emit('mic-start', channelId))
      session.on('mic-stop', (channelId: number) => this.emit('mic-stop', channelId))
      session.on('host-ui-requested', () => this.emit('host-ui-requested'))
      session.on('media-metadata', (m: MediaPlaybackMetadata) => this.emit('media-metadata', m))
      session.on('media-status', (s: MediaPlaybackStatus) => this.emit('media-status', s))
      session.on('connected', () => this.emit('connected'))
      session.on('disconnected', (reason?: string) => this.emit('disconnected', reason))
      session.on('error', (err: Error) => this.emit('error', err))

      this.emit('session', session)
    })

    this._server.on('error', (err: Error) => this.emit('error', err))
  }

  start(): void {
    this._server.listen(this._cfg.port)
  }

  stop(): void {
    this._server.close()
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

  sendVehicleEnergyModel(
    capacityWh: number,
    currentWh: number,
    rangeM: number,
    opts?: { maxChargePowerW?: number; maxDischargePowerW?: number; auxiliaryWhPerKm?: number }
  ): void {
    this._activeSession?.sendVehicleEnergyModel(capacityWh, currentWh, rangeM, opts)
  }

  sendMicPcm(buf: Buffer, ts?: bigint): void {
    this._activeSession?.sendMicPcm(buf, ts)
  }

  requestKeyframe(): void {
    this._activeSession?.requestKeyframe()
  }

  requestShutdown(): void {
    this._activeSession?.requestShutdown()
  }
}
