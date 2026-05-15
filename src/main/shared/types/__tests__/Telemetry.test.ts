import { isWired, routes, TELEMETRY_ROUTES, type TelemetryReceiver } from '../Telemetry'

describe('routes', () => {
  test('returns the declared entry for a known key', () => {
    expect(routes('speedKph')).toEqual(TELEMETRY_ROUTES.speedKph)
  })

  test('returns a fully-false fallback for an unknown key', () => {
    expect(routes('bogus')).toEqual({ dash: false, aa: false, dongle: false })
  })
})

describe('isWired', () => {
  test.each<[TelemetryReceiver, string, boolean]>([
    ['aa', 'speedKph', true],
    ['dongle', 'gps', true],
    ['dash', 'ts', true],
    ['aa', 'ts', false],
    ['dongle', 'speedKph', false]
  ])('isWired(%s, %s) → %s', (receiver, key, expected) => {
    expect(isWired(receiver, key)).toBe(expected)
  })

  test('returns false for an unknown key regardless of receiver', () => {
    expect(isWired('dash', 'unknown')).toBe(false)
    expect(isWired('aa', 'unknown')).toBe(false)
    expect(isWired('dongle', 'unknown')).toBe(false)
  })

  test('rejects "TODO" placeholder routes as not-wired', () => {
    // Find any TELEMETRY_ROUTES entry whose receiver value is 'TODO' (or
    // anything other than `true`). The semantic guarantee is: only literal
    // `true` resolves to wired.
    const allRoutes = Object.entries(TELEMETRY_ROUTES) as Array<
      [string, Record<TelemetryReceiver, unknown>]
    >
    for (const [key, route] of allRoutes) {
      for (const receiver of ['dash', 'aa', 'dongle'] as TelemetryReceiver[]) {
        const isTrue = route[receiver] === true
        expect(isWired(receiver, key)).toBe(isTrue)
      }
    }
  })
})
