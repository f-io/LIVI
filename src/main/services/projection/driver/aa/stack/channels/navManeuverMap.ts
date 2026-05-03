/**
 * Maps AA navigation events to LIVI's Carlinkit-compatible NaviBag codes so
 * the existing `translateNavigation` UI pipeline keeps working unchanged.
 *
 * AA's deprecated NextTurnEnum (used in INSTRUMENT_CLUSTER_NAVIGATION_TURN_EVENT)
 * is coarser than the Carlinkit ManeuverType (0–53). We pick the closest match
 * and use turn_side to disambiguate left/right variants where it exists.
 */

import type { NavigationTurnEvent, NavigationTurnSide } from './NavigationChannel.js'

/**
 * Returns the LIVI NaviManeuverType code (0–53) for a given AA turn-event +
 * turn-side combination. See translateNavigation.ts for the code → text table.
 */
export function turnEventToManeuverType(
  event: NavigationTurnEvent | undefined,
  side: NavigationTurnSide | undefined
): number | undefined {
  if (!event) return undefined
  const isLeft = side === 'left'
  const isRight = side === 'right'

  switch (event) {
    case 'unknown':
      return 0 // noTurn
    case 'depart':
      return 11 // proceedToRoute
    case 'name-change':
      return 5 // followRoad
    case 'slight-turn':
      return isRight ? 50 : 49 // slightRight / slightLeft
    case 'turn':
      return isRight ? 2 : 1 // right / left
    case 'sharp-turn':
      return isRight ? 48 : 47 // sharpRight / sharpLeft
    case 'u-turn':
      return 4 // uTurn
    case 'on-ramp':
      return 9 // rampOn
    case 'off-ramp':
      return isRight ? 23 : isLeft ? 22 : 8 // rampOffRight / rampOffLeft / rampOff
    case 'fork':
      return isRight ? 14 : 13 // keepRight / keepLeft
    case 'merge':
      return 9 // rampOn (closest match)
    case 'roundabout-enter':
      return 6 // enterRoundabout
    case 'roundabout-exit':
      return 7 // exitRoundabout
    case 'roundabout-enter-and-exit':
      return 6 // treat as enter; exit number isn't carried in deprecated event
    case 'straight':
      return 3 // straight
    case 'ferry-boat':
    case 'ferry-train':
      return 15 // enterFerry
    case 'destination':
      return isRight ? 25 : isLeft ? 24 : 12 // arrivedRight / arrivedLeft / arrived
    default:
      return 0
  }
}

/**
 * Carlinkit NaviTurnSide convention: 0 = right, 1 = left.
 * (See translateNavigation.ts: rawTurnSide === 0 → right, 1 → left.)
 */
export function turnSideToNaviCode(side: NavigationTurnSide | undefined): number | undefined {
  if (side === 'left') return 1
  if (side === 'right') return 0
  return undefined
}
