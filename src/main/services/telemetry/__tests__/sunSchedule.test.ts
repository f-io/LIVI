import type { Config } from '@shared/types'
import {
  attachSunSchedule,
  computeNightMode,
  isNightAt,
  resolveSunLocation,
  solarElevationDeg
} from '../sunSchedule'
import { TelemetryStore } from '../TelemetryStore'

const AMS = { lat: 52.37, lng: 4.9 }

describe('solar position', () => {
  test('midsummer noon in Amsterdam is high in the sky', () => {
    const elev = solarElevationDeg(new Date('2026-06-21T11:40:00Z'), AMS.lat, AMS.lng)
    expect(elev).toBeGreaterThan(58)
    expect(elev).toBeLessThan(63)
  })

  test('day / night around Amsterdam sunrise & sunset', () => {
    expect(isNightAt(new Date('2026-06-21T12:00:00Z'), AMS.lat, AMS.lng)).toBe(false)
    expect(isNightAt(new Date('2026-06-21T23:00:00Z'), AMS.lat, AMS.lng)).toBe(true)
    // Winter solstice: sunrise ~07:48 UTC, sunset ~15:29 UTC
    expect(isNightAt(new Date('2026-12-21T07:15:00Z'), AMS.lat, AMS.lng)).toBe(true)
    expect(isNightAt(new Date('2026-12-21T09:00:00Z'), AMS.lat, AMS.lng)).toBe(false)
    expect(isNightAt(new Date('2026-12-21T15:00:00Z'), AMS.lat, AMS.lng)).toBe(false)
    expect(isNightAt(new Date('2026-12-21T16:00:00Z'), AMS.lat, AMS.lng)).toBe(true)
  })
})

describe('resolveSunLocation', () => {
  test('prefers GPS over manual', () => {
    expect(resolveSunLocation({ lat: 1, lng: 2 }, { sunLatitude: 3, sunLongitude: 4 })).toEqual({
      lat: 1,
      lng: 2,
      source: 'gps'
    })
  })

  test('falls back to manual, treats 0/0 as unset', () => {
    expect(resolveSunLocation(undefined, { sunLatitude: 3, sunLongitude: 4 })?.source).toBe(
      'manual'
    )
    expect(resolveSunLocation({ lat: 0, lng: 0 }, { sunLatitude: 0, sunLongitude: 0 })).toBeNull()
    expect(resolveSunLocation({ lat: 95, lng: 2 }, undefined)).toBeNull()
  })
})

describe('computeNightMode', () => {
  test('without any location uses fixed 07:00–19:00 local daytime', () => {
    const at = (h: number) => {
      const d = new Date(2026, 5, 21, h, 0, 0)
      return computeNightMode(d, undefined, undefined)
    }
    expect(at(6)).toBe(true)
    expect(at(7)).toBe(false)
    expect(at(18)).toBe(false)
    expect(at(19)).toBe(true)
  })
})

describe('attachSunSchedule', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  const cfg = (over: Partial<Config>): Config =>
    ({ appearanceMode: 'sun', sunLatitude: AMS.lat, sunLongitude: AMS.lng, ...over }) as Config
  const trusted = { check: () => 'rtc' as const }

  test('sets nightMode immediately and re-evaluates every minute', () => {
    const store = new TelemetryStore()
    let now = new Date('2026-06-21T12:00:00Z')
    const h = attachSunSchedule({
      store,
      initialConfig: cfg({}),
      now: () => now,
      clockTrust: trusted
    })
    expect(store.snapshot().nightMode).toBe(false)

    now = new Date('2026-06-21T23:00:00Z')
    vi.advanceTimersByTime(60_000)
    expect(store.snapshot().nightMode).toBe(true)
    h.off()
  })

  test('does nothing outside sun mode and stops when switched away', () => {
    const store = new TelemetryStore()
    let now = new Date('2026-06-21T23:00:00Z')
    const h = attachSunSchedule({
      store,
      initialConfig: cfg({ appearanceMode: 'day' }),
      now: () => now,
      clockTrust: trusted
    })
    expect(store.snapshot().nightMode).toBeUndefined()

    h.setConfig(cfg({}))
    expect(store.snapshot().nightMode).toBe(true)

    h.setConfig(cfg({ appearanceMode: 'day' }))
    store.merge({ nightMode: false })
    now = new Date('2026-06-22T01:00:00Z')
    vi.advanceTimersByTime(120_000)
    expect(store.snapshot().nightMode).toBe(false)
    h.off()
  })

  test('holds last state until the clock is trusted, then switches', () => {
    const store = new TelemetryStore()
    store.merge({ nightMode: true })
    let source: 'ntp' | null = null
    const h = attachSunSchedule({
      store,
      initialConfig: cfg({}),
      now: () => new Date('2026-06-21T12:00:00Z'),
      clockTrust: { check: () => source }
    })
    expect(store.snapshot().nightMode).toBe(true)

    source = 'ntp'
    vi.advanceTimersByTime(60_000)
    expect(store.snapshot().nightMode).toBe(false)
    h.off()
  })
})
