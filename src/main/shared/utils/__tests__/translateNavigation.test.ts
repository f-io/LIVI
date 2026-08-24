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
    expect(result.TimeRemainingToDestinationText).toBe('1 min')
    expect(result.DistanceRemainingDisplayStringText).toBe('1.6 km')
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

  test('formats distances >= 10 km as whole kilometers', () => {
    const result = translateNavigation({ NaviDistanceToDestination: 12345 }, 'en')
    expect(result.DistanceRemainingDisplayStringText).toBe('12 km')
  })

  test('formats a multi-hour remaining time as h:mm with an h unit', () => {
    // 11 h 36 min → 41760 s (seconds dropped at this scale)
    const result = translateNavigation({ NaviTimeToDestination: 41760 }, 'en')
    expect(result.TimeRemainingToDestinationText).toBe('11:36 h')
  })

  test('passes through the arrival clock (NaviETA → EtaClockText)', () => {
    expect(translateNavigation({ NaviETA: '21:58' }, 'en').EtaClockText).toBe('21:58')
    expect(translateNavigation({}, 'en').EtaClockText).toBeUndefined()
  })

  test('formats 1km up to under 10km with one decimal', () => {
    expect(
      translateNavigation({ NaviDistanceToDestination: 1000 }, 'en')
        .DistanceRemainingDisplayStringText
    ).toBe('1.0 km')
    expect(
      translateNavigation({ NaviDistanceToDestination: 9100 }, 'en')
        .DistanceRemainingDisplayStringText
    ).toBe('9.1 km')
  })

  test('formats sub-kilometer distances in meters', () => {
    const result = translateNavigation({ NaviRemainDistance: 12.7 }, 'en')
    expect(result.RemainDistanceText).toBe('13 m')
  })

  test('supports dynamic roundabout exit labels', () => {
    const result = translateNavigation({ NaviManeuverType: 46 }, 'en')
    expect(result.ManeuverTypeText).toBe('Roundabout exit 19')
  })

  test('maps additional maneuver codes', () => {
    expect(translateNavigation({ NaviManeuverType: 0 }, 'en').ManeuverTypeText).toBe('No turn')
    expect(translateNavigation({ NaviManeuverType: 26 }, 'en').ManeuverTypeText).toBe(
      'Make a U-turn when possible'
    )
    expect(translateNavigation({ NaviManeuverType: 53 }, 'en').ManeuverTypeText).toBe(
      'Change highway (right)'
    )
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

  test('distances use a period decimal and no thousands separator on every locale', () => {
    for (const loc of ['en', 'de', 'ua'] as const) {
      // period decimal in the 1..10 km range
      expect(
        translateNavigation({ NaviDistanceToDestination: 9100 }, loc)
          .DistanceRemainingDisplayStringText
      ).toBe('9.1 km')
      // no thousands separator, no decimals at >= 10 km
      expect(
        translateNavigation({ NaviDistanceToDestination: 1065100 }, loc)
          .DistanceRemainingDisplayStringText
      ).toBe('1065 km')
    }
  })

  test('falls back to english dictionary for unsupported locale', () => {
    const result = translateNavigation({ NaviManeuverType: 2 }, 'xx' as never)
    expect(result.ManeuverTypeText).toBe('Turn right')
  })

  test('uses unknown for unsupported maneuver code', () => {
    const result = translateNavigation({ NaviManeuverType: 999 }, 'en')
    expect(result.ManeuverTypeText).toBe('Unknown')
  })

  test('returns undefined junction text for unsupported junction code', () => {
    const result = translateNavigation({ NaviJunctionType: 99 }, 'en')
    expect(result.JunctionTypeText).toBeUndefined()
    expect(result.codes.JunctionType).toBe(99)
  })

  test('keeps raw turn side code and derives driving side only for 0/1', () => {
    const result = translateNavigation({ NaviTurnSide: 5 }, 'en')
    expect(result.codes.TurnSide).toBe(5)
    expect(result.codes.DrivingSide).toBeUndefined()
    expect(result.DrivingSideText).toBeUndefined()
  })

  test('maps right-hand driving side from turn side 0', () => {
    const result = translateNavigation({ NaviTurnSide: 0 }, 'en')
    expect(result.codes.TurnSide).toBe(0)
    expect(result.codes.DrivingSide).toBe(0)
    expect(result.DrivingSideText).toBe('Right')
  })

  test('ignores non-string source fields and non-number code fields', () => {
    const result = translateNavigation(
      {
        NaviAPPName: 123,
        NaviDestinationName: false,
        NaviRoadName: {},
        NaviStatus: '2',
        NaviOrderType: '4',
        NaviTurnAngle: '35',
        NaviManeuverType: '1'
      } as never,
      'en'
    )

    expect(result.SourceName).toBeUndefined()
    expect(result.DestinationName).toBeUndefined()
    expect(result.CurrentRoadName).toBeUndefined()
    expect(result.ManeuverTypeText).toBe('Unknown')
    expect(result.codes.RouteGuidanceState).toBeUndefined()
    expect(result.codes.ManeuverState).toBeUndefined()
    expect(result.codes.TurnAngle).toBeUndefined()
    expect(result.codes.ManeuverType).toBeUndefined()
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

  test.each([
    [3, 'Go straight'],
    [4, 'Make a U-turn'],
    [5, 'Continue on the current road'],
    [6, 'Enter roundabout'],
    [7, 'Exit roundabout'],
    [8, 'Exit highway'],
    [9, 'Merge onto highway'],
    [10, 'End of navigation'],
    [11, 'Proceed to the route'],
    [12, 'Arrived'],
    [13, 'Keep left'],
    [14, 'Keep right'],
    [15, 'Enter ferry'],
    [16, 'Exit ferry'],
    [17, 'Change ferry'],
    [18, 'Make a U-turn to rejoin the route'],
    [19, 'Use the roundabout to make a U-turn'],
    [20, 'At the end of the road, turn left'],
    [21, 'At the end of the road, turn right'],
    [22, 'Exit highway on the left'],
    [23, 'Exit highway on the right'],
    [24, 'Arrived (left)'],
    [25, 'Arrived (right)'],
    [27, 'End of directions'],
    [49, 'Slight left'],
    [50, 'Slight right'],
    [51, 'Change highway'],
    [52, 'Change highway (left)']
  ])('maps maneuver code %i to %s', (code, expected) => {
    expect(translateNavigation({ NaviManeuverType: code }, 'en').ManeuverTypeText).toBe(expected)
  })

  test.each([28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46])(
    'maps roundabout exit code %i to the numbered exit text',
    (code) => {
      expect(translateNavigation({ NaviManeuverType: code }, 'en').ManeuverTypeText).toBe(
        `Roundabout exit ${code - 27}`
      )
    }
  )

  test('maps sharp turn and highway change codes', () => {
    expect(translateNavigation({ NaviManeuverType: 47 }, 'en').ManeuverTypeText).toBe('Sharp left')
    expect(translateNavigation({ NaviManeuverType: 48 }, 'en').ManeuverTypeText).toBe('Sharp right')
    expect(translateNavigation({ NaviManeuverType: 53 }, 'en').ManeuverTypeText).toBe(
      'Change highway (right)'
    )
  })

  test('passes NaviAfterRoadName through as AfterManeuverRoadName', () => {
    const result = translateNavigation({ NaviAfterRoadName: 'Elm St' }, 'en')
    expect(result.AfterManeuverRoadName).toBe('Elm St')
  })
})
