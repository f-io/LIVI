/**
 * Navigation status channel handler (CH.NAVIGATION = 12).
 *
 * Phone → HU only. Carries Google Maps turn-by-turn data so a HU with a
 * cluster / side widget can show maneuver + distance independently of the
 * main video stream. We translate it to LIVI's Carlinkit-compatible
 * `NaviBag` shape (Navi* keys) inside aaDriver.
 *
 * Message IDs (NavigationStatusMessageId.proto):
 *   0x8001 INSTRUMENT_CLUSTER_START          (StatusStart, empty)
 *   0x8002 INSTRUMENT_CLUSTER_STOP           (StatusStop,  empty)
 *   0x8003 INSTRUMENT_CLUSTER_NAVIGATION_STATUS    (NavigationStatus.status enum)
 *   0x8004 INSTRUMENT_CLUSTER_NAVIGATION_TURN_EVENT      [deprecated]
 *   0x8005 INSTRUMENT_CLUSTER_NAVIGATION_DISTANCE_EVENT  [deprecated]
 *   0x8006 INSTRUMENT_CLUSTER_NAVIGATION_STATE     (steps + destinations)
 *   0x8007 INSTRUMENT_CLUSTER_NAVIGATION_CURRENT_POSITION
 *
 * The deprecated TURN_EVENT / DISTANCE_EVENT pair is what current Maps
 * actually sends — modern STATE/CURRENT_POSITION are reserved for cluster
 * apps that aasdk hosts don't typically implement.
 */

import { EventEmitter } from 'node:events'
import { decodeFields, decodeVarintValue } from './protoEnc.js'

export const NAV_MSG = {
  START_INDICATION: 0x8001,
  STOP_INDICATION: 0x8002,
  STATUS: 0x8003,
  TURN_EVENT: 0x8004,
  DISTANCE_EVENT: 0x8005,
  STATE: 0x8006,
  CURRENT_POSITION: 0x8007
} as const

export type NavigationState = 'unavailable' | 'active' | 'inactive' | 'rerouting'

export type NavigationTurnSide = 'left' | 'right' | 'unspecified'

/** NextTurnEnum from NavigationNextTurnEvent.proto (deprecated TURN_EVENT). */
export type NavigationTurnEvent =
  | 'unknown'
  | 'depart'
  | 'name-change'
  | 'slight-turn'
  | 'turn'
  | 'sharp-turn'
  | 'u-turn'
  | 'on-ramp'
  | 'off-ramp'
  | 'fork'
  | 'merge'
  | 'roundabout-enter'
  | 'roundabout-exit'
  | 'roundabout-enter-and-exit'
  | 'straight'
  | 'ferry-boat'
  | 'ferry-train'
  | 'destination'

export interface NavigationStatusUpdate {
  state: NavigationState
}

export interface NavigationTurnUpdate {
  road?: string
  turnSide?: NavigationTurnSide
  event?: NavigationTurnEvent
  /** Raw turn-icon image bytes (PNG/bitmap). */
  image?: Buffer
  turnNumber?: number
  turnAngle?: number
}

export interface NavigationDistanceUpdate {
  distanceMeters: number
  timeToTurnSeconds: number
  /** Display value × 1000 in the unit indicated by displayUnit (e.g. 1.5 km = 1500). */
  displayDistanceE3?: number
  displayUnit?: number
}

export class NavigationChannel extends EventEmitter {
  // Events emitted:
  //   'nav-start'                            — instrument-cluster session began
  //   'nav-stop'                             — instrument-cluster session ended
  //   'nav-status'    (s: NavigationStatusUpdate)
  //   'nav-turn'      (t: NavigationTurnUpdate)
  //   'nav-distance'  (d: NavigationDistanceUpdate)

  handleMessage(msgId: number, payload: Buffer): void {
    switch (msgId) {
      case NAV_MSG.START_INDICATION:
        console.log('[NavigationChannel] START')
        this.emit('nav-start')
        break

      case NAV_MSG.STOP_INDICATION:
        console.log('[NavigationChannel] STOP')
        this.emit('nav-stop')
        break

      case NAV_MSG.STATUS: {
        const s = this._decodeStatus(payload)
        console.log(`[NavigationChannel] status=${s.state}`)
        this.emit('nav-status', s)
        break
      }

      case NAV_MSG.TURN_EVENT: {
        const t = this._decodeTurnEvent(payload)
        console.log(
          `[NavigationChannel] turn road=${JSON.stringify(t.road)} event=${t.event}` +
            ` side=${t.turnSide} angle=${t.turnAngle} image=${t.image ? `${t.image.length}B` : 'none'}`
        )
        this.emit('nav-turn', t)
        break
      }

      case NAV_MSG.DISTANCE_EVENT: {
        const d = this._decodeDistanceEvent(payload)
        console.log(
          `[NavigationChannel] distance ${d.distanceMeters}m t=${d.timeToTurnSeconds}s` +
            ` display=${d.displayDistanceE3}/${d.displayUnit}`
        )
        this.emit('nav-distance', d)
        break
      }

      case NAV_MSG.STATE:
      case NAV_MSG.CURRENT_POSITION:
        // Modern API: NavigationState / NavigationCurrentPosition. Maps doesn't
        // emit these to non-cluster HUs; left as a TODO if a future device does.
        console.debug(
          `[NavigationChannel] modern msgId=0x${msgId.toString(16)} len=${payload.length} (not parsed)`
        )
        break

      default:
        console.log(
          `[NavigationChannel] unhandled msgId=0x${msgId.toString(16)} len=${payload.length}`
        )
    }
  }

  private _decodeStatus(payload: Buffer): NavigationStatusUpdate {
    let raw = 0
    for (const f of decodeFields(payload)) {
      if (f.field === 1 && f.wire === 0) raw = decodeVarintValue(f.bytes)
    }
    const state: NavigationState =
      raw === 1 ? 'active' : raw === 2 ? 'inactive' : raw === 3 ? 'rerouting' : 'unavailable'
    return { state }
  }

  private _decodeTurnEvent(payload: Buffer): NavigationTurnUpdate {
    const out: NavigationTurnUpdate = {}
    for (const f of decodeFields(payload)) {
      switch (f.field) {
        case 1: // road (required string)
          out.road = f.bytes.toString('utf8')
          break
        case 2: {
          // turn_side (TurnSide enum: 1=LEFT, 2=RIGHT, 3=UNSPECIFIED)
          const v = decodeVarintValue(f.bytes)
          out.turnSide = v === 1 ? 'left' : v === 2 ? 'right' : 'unspecified'
          break
        }
        case 3: // event (NextTurnEnum)
          out.event = mapNextTurnEnum(decodeVarintValue(f.bytes))
          break
        case 4: // image (bytes)
          out.image = Buffer.from(f.bytes)
          break
        case 5: // turn_number
          out.turnNumber = decodeVarintValue(f.bytes)
          break
        case 6: // turn_angle
          out.turnAngle = decodeVarintValue(f.bytes)
          break
      }
    }
    return out
  }

  private _decodeDistanceEvent(payload: Buffer): NavigationDistanceUpdate {
    let distanceMeters = 0
    let timeToTurnSeconds = 0
    let displayDistanceE3: number | undefined
    let displayUnit: number | undefined
    for (const f of decodeFields(payload)) {
      switch (f.field) {
        case 1:
          distanceMeters = decodeVarintValue(f.bytes)
          break
        case 2:
          timeToTurnSeconds = decodeVarintValue(f.bytes)
          break
        case 3:
          displayDistanceE3 = decodeVarintValue(f.bytes)
          break
        case 4:
          displayUnit = decodeVarintValue(f.bytes)
          break
      }
    }
    return { distanceMeters, timeToTurnSeconds, displayDistanceE3, displayUnit }
  }
}

function mapNextTurnEnum(v: number): NavigationTurnEvent {
  switch (v) {
    case 1:
      return 'depart'
    case 2:
      return 'name-change'
    case 3:
      return 'slight-turn'
    case 4:
      return 'turn'
    case 5:
      return 'sharp-turn'
    case 6:
      return 'u-turn'
    case 7:
      return 'on-ramp'
    case 8:
      return 'off-ramp'
    case 9:
      return 'fork'
    case 10:
      return 'merge'
    case 11:
      return 'roundabout-enter'
    case 12:
      return 'roundabout-exit'
    case 13:
      return 'roundabout-enter-and-exit'
    case 14:
      return 'straight'
    case 16:
      return 'ferry-boat'
    case 17:
      return 'ferry-train'
    case 19:
      return 'destination'
    default:
      return 'unknown'
  }
}
