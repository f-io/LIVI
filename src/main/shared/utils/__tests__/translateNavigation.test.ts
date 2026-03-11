import { translateNavigation } from '@main/shared/utils'

describe('translateNavigation', () => {
  test('returns safe defaults when payload is null', () => {
    const result = translateNavigation(null, 'en')

    expect(result.SourceName).toBeUndefined()
    expect(result.DestinationName).toBeUndefined()
    expect(result.CurrentRoadName).toBeUndefined()
    expect(result.TimeRemainingToDestinationText).toBeUndefined()
    expect(result.DistanceRemainingDisplayStringText).toBeUndefined()
    expect(result.RemainDistanceText).toBeUndefined()
    expect(result.ManeuverTypeText).toBe('Unknown')
    expect(result.JunctionTypeText).toBeUndefined()
    expect(result.DrivingSideText).toBeUndefined()
    expect(result.codes).toEqual({
      RouteGuidanceState: undefined,
      ManeuverState: undefined,
      ManeuverType: undefined,
      TurnAngle: undefined,
      TurnSide: undefined,
      DrivingSide: undefined,
      JunctionType: undefined
    })
  })

  test('maps basic fields and english formatting', () => {
    const result = translateNavigation(
      {
        NaviAPPName: 'Maps',
        NaviDestinationName: 'Home',
        NaviRoadName: 'Main St',
        NaviTimeToDestination: 61.9,
        NaviDistanceToDestination: 1580,
        NaviRemainDistance: 999,
        NaviManeuverType: 1,
        NaviJunctionType: 1,
        NaviTurnSide: 1,
        NaviStatus: 2,
        NaviOrderType: 4,
        NaviTurnAngle: 35
      },
      'en'
    )

    expect(result.SourceName).toBe('Maps')
    expect(result.DestinationName).toBe('Home')
    expect(result.CurrentRoadName).toBe('Main St')
    expect(result.TimeRemainingToDestinationText).toBe('1:01')
    expect(result.DistanceRemainingDisplayStringText).toBe('1.58 km')
    expect(result.RemainDistanceText).toBe('999 m')
    expect(result.ManeuverTypeText).toBe('Turn left')
    expect(result.JunctionTypeText).toBe('Roundabout')
    expect(result.DrivingSideText).toBe('Left')
    expect(result.codes).toEqual({
      RouteGuidanceState: 2,
      ManeuverState: 4,
      ManeuverType: 1,
      TurnAngle: 35,
      TurnSide: 1,
      DrivingSide: 1,
      JunctionType: 1
    })
  })

  test('formats large distances with one decimal', () => {
    const result = translateNavigation({ NaviDistanceToDestination: 12345 }, 'en')
    expect(result.DistanceRemainingDisplayStringText).toBe('12.3 km')
  })

  test('supports dynamic roundabout exit labels', () => {
    const result = translateNavigation({ NaviManeuverType: 46 }, 'en')
    expect(result.ManeuverTypeText).toBe('Roundabout exit 19')
  })

  test('uses locale dictionaries for de and ua', () => {
    const de = translateNavigation(
      { NaviManeuverType: 47, NaviTurnSide: 0, NaviJunctionType: 0 },
      'de'
    )
    const ua = translateNavigation({ NaviManeuverType: 48 }, 'ua')

    expect(de.ManeuverTypeText).toBe('Scharf links')
    expect(de.DrivingSideText).toBe('Rechtsverkehr')
    expect(de.JunctionTypeText).toBe('Kreuzung')
    expect(ua.ManeuverTypeText).toBe('Різко праворуч')
  })

  test('falls back to english dictionary for unsupported locale', () => {
    const result = translateNavigation({ NaviManeuverType: 2 }, 'fr' as never)
    expect(result.ManeuverTypeText).toBe('Turn right')
  })

  test('uses unknown for unsupported maneuver code', () => {
    const result = translateNavigation({ NaviManeuverType: 999 }, 'en')
    expect(result.ManeuverTypeText).toBe('Unknown')
  })

  test('keeps raw turn side code and derives driving side only for 0/1', () => {
    const result = translateNavigation({ NaviTurnSide: 5 }, 'en')
    expect(result.codes.TurnSide).toBe(5)
    expect(result.codes.DrivingSide).toBeUndefined()
    expect(result.DrivingSideText).toBeUndefined()
  })

  test('does not format invalid numeric values', () => {
    const result = translateNavigation(
      {
        NaviTimeToDestination: -10,
        NaviDistanceToDestination: Number.NaN,
        NaviRemainDistance: Number.POSITIVE_INFINITY
      },
      'en'
    )

    expect(result.TimeRemainingToDestinationText).toBeUndefined()
    expect(result.DistanceRemainingDisplayStringText).toBeUndefined()
    expect(result.RemainDistanceText).toBeUndefined()
  })
})
