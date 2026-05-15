import { turnEventToManeuverType, turnSideToNaviCode } from '../navManeuverMap'

describe('turnEventToManeuverType', () => {
  test('returns undefined when event is missing', () => {
    expect(turnEventToManeuverType(undefined, undefined)).toBeUndefined()
    expect(turnEventToManeuverType(undefined, 'left')).toBeUndefined()
  })

  test.each([
    ['unknown', undefined, 0],
    ['depart', undefined, 11],
    ['name-change', undefined, 5],
    ['u-turn', undefined, 4],
    ['on-ramp', undefined, 9],
    ['merge', undefined, 9],
    ['roundabout-enter', undefined, 6],
    ['roundabout-exit', undefined, 7],
    ['roundabout-enter-and-exit', undefined, 6],
    ['straight', undefined, 3],
    ['ferry-boat', undefined, 15],
    ['ferry-train', undefined, 15]
  ])('event=%s side=%s → %s', (event, side, expected) => {
    expect(turnEventToManeuverType(event as never, side as never)).toBe(expected)
  })

  test('slight-turn picks right/left based on side', () => {
    expect(turnEventToManeuverType('slight-turn', 'right')).toBe(50)
    expect(turnEventToManeuverType('slight-turn', 'left')).toBe(49)
    expect(turnEventToManeuverType('slight-turn', 'unspecified')).toBe(49)
  })

  test('turn picks right/left based on side', () => {
    expect(turnEventToManeuverType('turn', 'right')).toBe(2)
    expect(turnEventToManeuverType('turn', 'left')).toBe(1)
    expect(turnEventToManeuverType('turn', 'unspecified')).toBe(1)
  })

  test('sharp-turn picks right/left based on side', () => {
    expect(turnEventToManeuverType('sharp-turn', 'right')).toBe(48)
    expect(turnEventToManeuverType('sharp-turn', 'left')).toBe(47)
  })

  test('off-ramp has three branches: right, left, none', () => {
    expect(turnEventToManeuverType('off-ramp', 'right')).toBe(23)
    expect(turnEventToManeuverType('off-ramp', 'left')).toBe(22)
    expect(turnEventToManeuverType('off-ramp', 'unspecified')).toBe(8)
  })

  test('fork picks right/left based on side', () => {
    expect(turnEventToManeuverType('fork', 'right')).toBe(14)
    expect(turnEventToManeuverType('fork', 'left')).toBe(13)
  })

  test('destination has three branches: right, left, none', () => {
    expect(turnEventToManeuverType('destination', 'right')).toBe(25)
    expect(turnEventToManeuverType('destination', 'left')).toBe(24)
    expect(turnEventToManeuverType('destination', 'unspecified')).toBe(12)
  })

  test('falls back to 0 for an unrecognized event string', () => {
    expect(turnEventToManeuverType('bogus' as never, undefined)).toBe(0)
  })
})

describe('turnSideToNaviCode', () => {
  test('right → 0, left → 1, unspecified → undefined', () => {
    expect(turnSideToNaviCode('right')).toBe(0)
    expect(turnSideToNaviCode('left')).toBe(1)
    expect(turnSideToNaviCode('unspecified')).toBeUndefined()
    expect(turnSideToNaviCode(undefined)).toBeUndefined()
  })
})
