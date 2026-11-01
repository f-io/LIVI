/**
 * Clock trust — decides whether the system wall clock can be used for
 * sunrise/sunset night mode.
 *
 * Raspberry Pi 4 has no RTC and loses the time on every power-off; the Pi 5
 * has an RTC that keeps time only with a backup battery fitted. Without a
 * trusted clock the sun schedule would flip day/night at the wrong moment, so
 * it holds the last known state until one of these confirms the time:
 *
 *   1. rtc     — /sys/class/rtc/rtc0 holds a plausible time close to system time
 *   2. ntp     — systemd-timesyncd marker (/run/systemd/timesync/synchronized)
 *   3. ntp     — chrony reports a normal leap status (`chronyc -c tracking`)
 *
 * Once trusted, the clock stays trusted for the rest of the session.
 * Non-Linux hosts (dev machines) are always trusted.
 */

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'

/** Anything before this means "clock never set" (Pi without RTC/NTP boots near 1970). */
export const MIN_PLAUSIBLE_CLOCK_MS = Date.UTC(2026, 0, 1)
/** RTC and system clock must agree within this window to count as RTC-backed. */
const RTC_MAX_SKEW_MS = 5 * 60_000

const RTC_EPOCH_PATH = '/sys/class/rtc/rtc0/since_epoch'
const TIMESYNCD_MARKER = '/run/systemd/timesync/synchronized'

export type ClockSource = 'rtc' | 'ntp' | 'host'

export type ClockTrustDeps = {
  platform?: NodeJS.Platform
  now?: () => number
  readFile?: (path: string) => string | null
  fileExists?: (path: string) => boolean
  chronyTracking?: () => string | null
}

export type ClockTrust = {
  /** Source that confirmed the clock, or null while untrusted. Re-checks until trusted. */
  check: () => ClockSource | null
}

export function isClockPlausible(ms: number): boolean {
  return ms >= MIN_PLAUSIBLE_CLOCK_MS
}

const defaultReadFile = (path: string): string | null => {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return null
  }
}

const defaultChronyTracking = (): string | null => {
  try {
    return execFileSync('chronyc', ['-n', '-c', 'tracking'], {
      encoding: 'utf8',
      timeout: 1000,
      stdio: ['ignore', 'pipe', 'ignore']
    })
  } catch {
    return null
  }
}

export function createClockTrust({
  platform = process.platform,
  now = () => Date.now(),
  readFile = defaultReadFile,
  fileExists = existsSync,
  chronyTracking = defaultChronyTracking
}: ClockTrustDeps = {}): ClockTrust {
  let trusted: ClockSource | null = platform === 'linux' ? null : 'host'
  let logged = false

  const rtcOk = (sysMs: number): boolean => {
    const raw = readFile(RTC_EPOCH_PATH)
    if (raw == null) return false
    const rtcMs = Number.parseInt(raw.trim(), 10) * 1000
    return (
      Number.isFinite(rtcMs) &&
      isClockPlausible(rtcMs) &&
      Math.abs(rtcMs - sysMs) <= RTC_MAX_SKEW_MS
    )
  }

  const chronyOk = (): boolean => {
    const out = chronyTracking()
    if (!out) return false
    // CSV: ref id, ref name, stratum, ref time, ..., leap status (last field)
    const fields = out.trim().split(',')
    const stratum = Number.parseInt(fields[2] ?? '', 10)
    const leap = fields[fields.length - 1]?.trim()
    return Number.isFinite(stratum) && stratum > 0 && stratum < 16 && leap === 'Normal'
  }

  return {
    check: () => {
      if (trusted) return trusted
      const sysMs = now()
      if (!isClockPlausible(sysMs)) return null

      if (rtcOk(sysMs)) trusted = 'rtc'
      else if (fileExists(TIMESYNCD_MARKER) || chronyOk()) trusted = 'ntp'

      if (trusted) {
        console.log(`[clockTrust] system clock trusted (source: ${trusted})`)
      } else if (!logged) {
        logged = true
        console.warn(
          '[clockTrust] clock not confirmed by RTC or NTP — sunrise/sunset night mode on hold'
        )
      }
      return trusted
    }
  }
}
