/**
 * Sunrise / sunset driven night mode.
 *
 * When `appearanceMode === 'sun'`, nightMode is derived from the sun's
 * elevation at the vehicle's position. The result flows through the regular
 * TelemetryStore, so LIVI UI (red accent), Android Auto, CarPlay and the
 * dongle all switch together.
 *
 * Location priority:
 *   1. GPS fix in the telemetry store (live or persisted lastKnownGps)
 *   2. Manually configured sunLatitude / sunLongitude
 *   3. Fallback fixed hours (day = 07:00–19:00 local time)
 *
 * The schedule only acts once the wall clock is trusted (RTC or NTP, see
 * clockTrust.ts). Until then the last known night state is kept.
 */

import type { Config } from '@shared/types'
import { type ClockTrust, createClockTrust } from './clockTrust'
import type { TelemetryStore } from './TelemetryStore'

const TICK_MS = 60_000
/** Standard sunrise/sunset altitude: refraction + solar disc radius. */
const SUNSET_ALTITUDE_DEG = -0.833
const FALLBACK_DAY_START_H = 7
const FALLBACK_DAY_END_H = 19

const rad = (deg: number): number => (deg * Math.PI) / 180
const deg = (r: number): number => (r * 180) / Math.PI

/** Solar elevation angle in degrees (NOAA low-precision formula, ~0.1° accuracy). */
export function solarElevationDeg(date: Date, lat: number, lng: number): number {
  const d = date.getTime() / 86_400_000 + 2_440_587.5 - 2_451_545.0
  const g = rad(357.529 + 0.98560028 * d)
  const q = 280.459 + 0.98564736 * d
  const L = rad(q + 1.915 * Math.sin(g) + 0.02 * Math.sin(2 * g))
  const e = rad(23.439 - 0.00000036 * d)

  const ra = Math.atan2(Math.cos(e) * Math.sin(L), Math.cos(L))
  const dec = Math.asin(Math.sin(e) * Math.sin(L))

  const gmstDeg = (18.697374558 + 24.06570982441908 * d) * 15
  const ha = rad(gmstDeg + lng) - ra

  const latR = rad(lat)
  return deg(
    Math.asin(Math.sin(latR) * Math.sin(dec) + Math.cos(latR) * Math.cos(dec) * Math.cos(ha))
  )
}

export function isNightAt(date: Date, lat: number, lng: number): boolean {
  return solarElevationDeg(date, lat, lng) < SUNSET_ALTITUDE_DEG
}

export function isValidLatLng(lat: unknown, lng: unknown): lat is number {
  return (
    typeof lat === 'number' &&
    typeof lng === 'number' &&
    Number.isFinite(lat) &&
    Number.isFinite(lng) &&
    Math.abs(lat) <= 90 &&
    Math.abs(lng) <= 180 &&
    !(lat === 0 && lng === 0) // 0/0 = "not configured"
  )
}

export type SunLocation = { lat: number; lng: number; source: 'gps' | 'manual' }

export function resolveSunLocation(
  gps: { lat?: number; lng?: number } | undefined,
  cfg: Pick<Config, 'sunLatitude' | 'sunLongitude'> | undefined
): SunLocation | null {
  if (gps && isValidLatLng(gps.lat, gps.lng)) {
    return { lat: gps.lat as number, lng: gps.lng as number, source: 'gps' }
  }
  if (cfg && isValidLatLng(cfg.sunLatitude, cfg.sunLongitude)) {
    return { lat: cfg.sunLatitude as number, lng: cfg.sunLongitude as number, source: 'manual' }
  }
  return null
}

export function computeNightMode(
  now: Date,
  gps: { lat?: number; lng?: number } | undefined,
  cfg: Pick<Config, 'sunLatitude' | 'sunLongitude'> | undefined
): boolean {
  const loc = resolveSunLocation(gps, cfg)
  if (loc) return isNightAt(now, loc.lat, loc.lng)
  const h = now.getHours()
  return h < FALLBACK_DAY_START_H || h >= FALLBACK_DAY_END_H
}

export type SunScheduleDeps = {
  store: TelemetryStore
  initialConfig?: Config
  now?: () => Date
  clockTrust?: ClockTrust
}

export type SunScheduleHandle = {
  setConfig: (cfg: Config) => void
  off: () => void
}

export function attachSunSchedule({
  store,
  initialConfig,
  now = () => new Date(),
  clockTrust = createClockTrust({ now: () => now().getTime() })
}: SunScheduleDeps): SunScheduleHandle {
  let cfg: Config | undefined = initialConfig
  let timer: ReturnType<typeof setInterval> | null = null

  const evaluate = (): void => {
    if (cfg?.appearanceMode !== 'sun') return
    if (!clockTrust.check()) return
    const at = now()
    const night = computeNightMode(at, store.snapshot().gps, cfg)
    if (store.snapshot().nightMode !== night) store.merge({ nightMode: night })
  }

  const sync = (): void => {
    const active = cfg?.appearanceMode === 'sun'
    if (active && !timer) {
      timer = setInterval(evaluate, TICK_MS)
    } else if (!active && timer) {
      clearInterval(timer)
      timer = null
    }
    if (active) evaluate()
  }

  sync()

  return {
    setConfig: (next) => {
      cfg = next
      sync()
    },
    off: () => {
      if (timer) clearInterval(timer)
      timer = null
    }
  }
}
