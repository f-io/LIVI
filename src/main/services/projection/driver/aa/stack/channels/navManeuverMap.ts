/**
 * Maps AA navigation events to LIVI's NaviBag codes
 *
 * AA's deprecated NextTurnEnum (used in INSTRUMENT_CLUSTER_NAVIGATION_TURN_EVENT)
 * is coarser than the ManeuverType (0–53). We pick the closest match
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
 */
export function turnSideToNaviCode(side: NavigationTurnSide | undefined): number | undefined {
  if (side === 'left') return 1
  if (side === 'right') return 0
  return undefined
}

export function navManeuverTypeToCode(type: number | undefined): number | undefined {
  switch (type) {
    case 0:
      return 0 // UNKNOWN → noTurn
    case 1:
      return 11 // DEPART → proceedToRoute
    case 2:
      return 5 // NAME_CHANGE → followRoad
    case 3:
      return 13 // KEEP_LEFT
    case 4:
      return 14 // KEEP_RIGHT
    case 5:
      return 49 // TURN_SLIGHT_LEFT → slightLeft
    case 6:
      return 50 // TURN_SLIGHT_RIGHT → slightRight
    case 7:
      return 1 // TURN_NORMAL_LEFT → left
    case 8:
      return 2 // TURN_NORMAL_RIGHT → right
    case 9:
      return 47 // TURN_SHARP_LEFT → sharpLeft
    case 10:
      return 48 // TURN_SHARP_RIGHT → sharpRight
    case 11:
    case 12:
      return 4 // U_TURN_* → uTurn
    case 13:
    case 14:
    case 15:
    case 16:
    case 17:
    case 18:
    case 19:
    case 20:
      return 9 // ON_RAMP_* → rampOn
    case 21:
    case 23:
      return 22 // OFF_RAMP_*_LEFT → rampOffLeft
    case 22:
    case 24:
      return 23 // OFF_RAMP_*_RIGHT → rampOffRight
    case 25:
      return 13 // FORK_LEFT → keepLeft
    case 26:
      return 14 // FORK_RIGHT → keepRight
    case 27:
    case 28:
    case 29:
      return 9 // MERGE_* → rampOn (closest)
    case 30:
      return 6 // ROUNDABOUT_ENTER
    case 31:
      return 7 // ROUNDABOUT_EXIT
    case 32:
    case 33:
    case 34:
    case 35:
      return 6 // ROUNDABOUT_ENTER_AND_EXIT_* → enterRoundabout
    case 36:
      return 3 // STRAIGHT
    case 37:
    case 38:
      return 15 // FERRY_* → enterFerry
    case 39:
    case 40:
      return 12 // DESTINATION(_STRAIGHT) → arrived
    case 41:
      return 24 // DESTINATION_LEFT → arrivedLeft
    case 42:
      return 25 // DESTINATION_RIGHT → arrivedRight
    default:
      return undefined
  }
}

/** Carlinkit turn-side (0=right, 1=left) inferred from the modern maneuver enum. */
export function navManeuverTypeToSide(type: number | undefined): number | undefined {
  switch (type) {
    case 3: // KEEP_LEFT
    case 5: // TURN_SLIGHT_LEFT
    case 7: // TURN_NORMAL_LEFT
    case 9: // TURN_SHARP_LEFT
    case 11: // U_TURN_LEFT
    case 25: // FORK_LEFT
    case 41: // DESTINATION_LEFT
      return 1
    case 4: // KEEP_RIGHT
    case 6: // TURN_SLIGHT_RIGHT
    case 8: // TURN_NORMAL_RIGHT
    case 10: // TURN_SHARP_RIGHT
    case 12: // U_TURN_RIGHT
    case 26: // FORK_RIGHT
    case 42: // DESTINATION_RIGHT
      return 0
    default:
      return undefined
  }
}
