// Host timezone. GPS gives UTC only, so the zone comes from the position; an iPhone's
// reported offset is the fallback. Whole-hour offsets map to DST-free Etc/GMT±N.

import { execFile } from 'node:child_process'
import fs from 'node:fs'
import tzLookup from './vendor/tzLookup'

const SET_TIME_HELPER = '/usr/local/lib/livi/livi-set-time.sh'

const WINTER = Date.UTC(2026, 0, 15)
const SUMMER = Date.UTC(2026, 6, 15)

/** Minutes east of UTC for a zone at a given instant. */
export function zoneOffsetMinutes(zone: string, at: number): number | null {
  try {
    const name = new Intl.DateTimeFormat('en-US', {
      timeZone: zone,
      timeZoneName: 'longOffset'
    }).format(new Date(at))
    // A bad zone throws above. Older ICU renders a zero offset as a bare "GMT".
    const m = /GMT([+-])(\d{2}):(\d{2})/.exec(name)
    if (!m) return /GMT(?![+-])/.test(name) ? 0 : null
    return (m[1] === '-' ? -1 : 1) * (Number(m[2]) * 60 + Number(m[3]))
  } catch {
    return null
  }
}

/** True when the zone shifts between winter and summer. */
export function observesDst(zone: string): boolean {
  const w = zoneOffsetMinutes(zone, WINTER)
  const s = zoneOffsetMinutes(zone, SUMMER)
  return w !== null && s !== null && w !== s
}

let zoneCache: string[] | null = null
function allZones(): string[] {
  zoneCache ??= Intl.supportedValuesOf('timeZone')
  return zoneCache
}

/** Every zone the runtime knows, for the manual picker. */
export function listTimezones(): string[] {
  return [...allZones()].sort()
}

/** Zone for a reported offset in minutes east of UTC, preferring one without DST. */
export function resolveZoneForOffset(offsetMinutes: number, now = Date.now()): string | null {
  if (!Number.isFinite(offsetMinutes)) return null
  if (Math.abs(offsetMinutes) > 14 * 60) return null

  if (offsetMinutes % 60 === 0) {
    const hours = offsetMinutes / 60
    // POSIX inverts the sign: UTC+2 is Etc/GMT-2
    return hours === 0 ? 'UTC' : `Etc/GMT${hours > 0 ? '-' : '+'}${Math.abs(hours)}`
  }

  let fallback: string | null = null
  for (const zone of allZones()) {
    if (zoneOffsetMinutes(zone, now) !== offsetMinutes) continue
    if (!observesDst(zone)) return zone
    fallback ??= zone
  }
  return fallback
}

/** Zone for a GNSS position. Names a real zone, so DST comes from the tz database. */
export function zoneForPosition(lat: number, lng: number): string | null {
  if (!Number.isFinite(lat) || !Number.isFinite(lng)) return null
  if (Math.abs(lat) > 90 || Math.abs(lng) > 180) return null
  return tzLookup(lat, lng)
}

/** The zone the host is currently set to. */
export function currentZone(): string {
  return Intl.DateTimeFormat().resolvedOptions().timeZone
}

/** Apply via the root helper. No helper = installer has not run; the zone stays. */
export function applyTimezone(zone: string): void {
  if (zone === currentZone()) return
  if (zoneOffsetMinutes(zone, Date.now()) === null) {
    console.warn(`[timezone] ${zone} is not a zone this system knows`)
    return
  }
  if (!fs.existsSync(SET_TIME_HELPER)) {
    console.warn('[timezone] helper missing — re-run the installer to let LIVI set the zone')
    return
  }
  execFile('sudo', ['-n', SET_TIME_HELPER, 'tz', zone], (err) => {
    if (err) console.warn('[timezone] could not set the zone:', err.message)
    else console.log(`[timezone] host zone → ${zone}`)
  })
}
