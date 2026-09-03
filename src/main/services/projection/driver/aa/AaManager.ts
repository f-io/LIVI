/**
 * AaManager, shared Android Auto infrastructure (singleton).
 *
 * The helper carries every phone, over WiFi after the WPP bootstrap and over USB
 * after the AOAP switch, and announces each session. Every announcement spawns ONE
 * AaSession handed off via onSpawn. Holds the codec-capability seed applied to each
 * new AaSession.
 */

import type { Config } from '@shared/types'
import type { AaMediaSinkDeps } from './AaEventBridge'
import { AaSession, type AaSessionSeed } from './AaSession'
import { HelperSessionLink } from './stack/transport/HelperSessionLink'

export type HelperSessionEvent = {
  event: string
  socket?: string
  peer?: string
  transport?: string
  serial?: string
  product?: number
  version?: number
  name?: string
}

/** The helper's event stream, where new phone sessions are announced. */
export type HelperSessionSource = {
  subscribe(onEvent: (ev: HelperSessionEvent) => void, onClose?: () => void): { close: () => void }
}

const HELPER_RESUBSCRIBE_MS = 2000

export interface AaManagerOptions {
  getConfig: () => Config
  onSpawn: (session: AaSession) => void
  mediaSink?: AaMediaSinkDeps
}

export class AaManager {
  private _helper: HelperSessionSource | null = null
  private _helperSub: { close: () => void } | null = null
  private readonly _sessions = new Set<AaSession>()
  private readonly _wirelessPeer = new Map<AaSession, string>()

  private _hevcSupported = false
  private _vp9Supported = false
  private _av1Supported = false
  private _initialNightMode: boolean | undefined = undefined
  private _clusterStreamActive = true

  private readonly _getConfig: () => Config
  private readonly _onSpawn: (session: AaSession) => void
  private readonly _mediaSink: AaMediaSinkDeps | undefined

  constructor(opts: AaManagerOptions) {
    this._getConfig = opts.getConfig
    this._onSpawn = opts.onSpawn
    this._mediaSink = opts.mediaSink
  }

  setHevcSupported(supported: boolean): void {
    this._hevcSupported = supported
    for (const s of this._sessions) s.setHevcSupported(supported)
  }

  setVp9Supported(supported: boolean): void {
    this._vp9Supported = supported
    for (const s of this._sessions) s.setVp9Supported(supported)
  }

  setAv1Supported(supported: boolean): void {
    this._av1Supported = supported
    for (const s of this._sessions) s.setAv1Supported(supported)
  }

  setInitialNightMode(value: boolean | undefined): void {
    this._initialNightMode = value
    for (const s of this._sessions) s.setInitialNightMode(value)
  }

  // ── Telemetry fan-out: every connected session, like CpManager ──────────────
  sendSpeedData(speedMmS: number, cruiseEngaged?: boolean, cruiseSetSpeedMmS?: number): void {
    for (const s of this._sessions) s.sendSpeedData(speedMmS, cruiseEngaged, cruiseSetSpeedMmS)
  }
  sendRpmData(rpmE3: number): void {
    for (const s of this._sessions) s.sendRpmData(rpmE3)
  }
  sendGearData(gear: number): void {
    for (const s of this._sessions) s.sendGearData(gear)
  }
  sendNightModeData(nightMode: boolean): void {
    for (const s of this._sessions) s.sendNightModeData(nightMode)
  }
  sendParkingBrakeData(engaged: boolean): void {
    for (const s of this._sessions) s.sendParkingBrakeData(engaged)
  }
  sendDrivingStatusData(status: number): void {
    for (const s of this._sessions) s.sendDrivingStatusData(status)
  }
  sendLightData(headLight?: 1 | 2 | 3, hazardLights?: boolean, turnIndicator?: 1 | 2 | 3): void {
    for (const s of this._sessions) s.sendLightData(headLight, hazardLights, turnIndicator)
  }
  sendFuelData(level: number, range?: number, lowFuelWarning?: boolean): void {
    for (const s of this._sessions) s.sendFuelData(level, range, lowFuelWarning)
  }
  sendOdometerData(totalKmE1: number, tripKmE1?: number): void {
    for (const s of this._sessions) s.sendOdometerData(totalKmE1, tripKmE1)
  }
  sendEnvironmentData(temperatureE3?: number, pressureE3?: number, rain?: number): void {
    for (const s of this._sessions) s.sendEnvironmentData(temperatureE3, pressureE3, rain)
  }
  sendGpsLocationData(opts: {
    latDeg: number
    lngDeg: number
    accuracyM?: number
    altitudeM?: number
    speedMs?: number
    bearingDeg?: number
  }): void {
    for (const s of this._sessions) s.sendGpsLocationData(opts)
  }
  sendVehicleEnergyModel(
    capacityWh: number,
    currentWh: number,
    rangeM: number,
    opts?: { maxChargePowerW?: number; maxDischargePowerW?: number; auxiliaryWhPerKm?: number }
  ): void {
    for (const s of this._sessions) s.sendVehicleEnergyModel(capacityWh, currentWh, rangeM, opts)
  }

  setClusterStreamActive(active: boolean): void {
    this._clusterStreamActive = active
    for (const s of this._sessions) s.setClusterStreamActive(active)
  }

  private _seed(): AaSessionSeed {
    return {
      hevcSupported: this._hevcSupported,
      vp9Supported: this._vp9Supported,
      av1Supported: this._av1Supported,
      initialNightMode: this._initialNightMode,
      clusterStreamActive: this._clusterStreamActive
    }
  }

  // ── Sessions from the helper ───────────────────────────────────────────────

  attachHelper(helper: HelperSessionSource | undefined): void {
    if (this._helper) return
    if (!helper) {
      console.warn('[AaManager] Android Auto needs the helper, none is running')
      return
    }
    this._helper = helper
    this._openHelperSub()
    console.log('[AaManager] Android Auto sessions come from the helper')
  }

  private _openHelperSub(): void {
    const helper = this._helper
    if (!helper) return
    this._helperSub = helper.subscribe(
      (ev) => {
        if (ev.event !== 'aa-session' || typeof ev.socket !== 'string') return
        const socket = ev.socket
        const peer = typeof ev.peer === 'string' ? ev.peer : ''
        const wired = ev.transport === 'usb'
        const usbSerial = wired && typeof ev.serial === 'string' ? ev.serial : undefined
        console.log(`[AaManager] helper session ${socket} from ${peer}${wired ? ' (usb)' : ''}`)
        HelperSessionLink.connect(socket, peer)
          .then((link) => {
            if (!this._helper) {
              link.destroy()
              return
            }
            if (!wired) this._supersedeWireless(peer)
            this._spawn(link, wired, usbSerial, wired ? undefined : peer)
          })
          .catch((err: Error) => {
            console.warn(`[AaManager] helper session ${socket}: ${err.message}`)
          })
      },
      () => {
        this._helperSub = null
        if (this._helper) setTimeout(() => this._openHelperSub(), HELPER_RESUBSCRIBE_MS)
      }
    )
  }

  private _closeHelperSub(): void {
    this._helper = null
    const sub = this._helperSub
    this._helperSub = null
    try {
      sub?.close()
    } catch {
      /* already closed */
    }
  }

  /** The helper is gone, and with it every session it carried. */
  detachHelper(): void {
    this._closeHelperSub()
    for (const s of [...this._sessions]) void s.close()
  }

  /** Wireless AA was switched off. Its sessions end, the USB ones stay. */
  stopWireless(): void {
    for (const s of [...this._sessions]) {
      if (!s.isWiredMode()) void s.close()
    }
  }

  async close(): Promise<void> {
    this._closeHelperSub()
    const sessions = [...this._sessions]
    this._sessions.clear()
    await Promise.all(
      sessions.map((s) =>
        s
          .close()
          .catch((e) =>
            console.warn(`[AaManager] session close threw on close: ${(e as Error).message}`)
          )
      )
    )
  }

  private _spawn(
    link: HelperSessionLink,
    wired: boolean,
    usbSerial: string | undefined,
    wirelessIp?: string
  ): void {
    const session = new AaSession({
      transport: link,
      getConfig: this._getConfig,
      wired,
      usbSerial,
      seed: this._seed(),
      mediaSink: this._mediaSink
    })
    this._sessions.add(session)
    if (wirelessIp) this._wirelessPeer.set(session, wirelessIp)
    session.once('disconnected', () => {
      this._sessions.delete(session)
      this._wirelessPeer.delete(session)
    })
    this._onSpawn(session)
  }

  // Close any existing wireless session from the same phone-IP.
  private _supersedeWireless(ip: string): void {
    if (!ip) return
    for (const [session, peer] of this._wirelessPeer) {
      if (peer === ip) {
        console.log(`[AaManager] wireless reconnect from ${ip}, dropping the superseded session`)
        void session.close()
      }
    }
  }
}

export default AaManager
