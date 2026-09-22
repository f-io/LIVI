// Sets the system clock from GNSS time. A marker file claims the clock so the helper
// (separate root process) skips its own step while GPS holds it.

import { execFile } from 'node:child_process'
import fs from 'node:fs'
import type { GnssInfo } from '@shared/types/Gnss'

/** Checked by the helper before it steps the clock from the phone's time. */
export const CLOCK_CLAIM_FILE = '/tmp/livi-gps-clock'
/** Root helper, installed by the LIVI installers. */
const SET_TIME_HELPER = '/usr/local/lib/livi/livi-set-time.sh'

/** Drift below this is left alone. */
const DRIFT_THRESHOLD_S = 2
/** Above this the clock is plainly wrong, so satellite time alone is enough. */
const GROSS_DRIFT_S = 60
/** Minimum interval between steps. */
const MIN_STEP_INTERVAL_MS = 60_000
/** Claim refresh interval. */
const CLAIM_REFRESH_MS = 30_000

/** True for a 3D lock on enough satellites with usable geometry. */
export function isTimeSourceTrustworthy(info: GnssInfo): boolean {
  if (info.receiverTime === undefined) return false
  if (info.fixMode !== '3d') return false
  if (info.satellitesUsed < 4) return false
  return info.hdop === undefined || info.hdop <= 5
}

/**
 * The receiver decodes time from the signal minutes before it can fix a position,
 * so a plainly wrong clock is corrected from that alone. Fine corrections still
 * wait for a proper lock.
 */
function mayStep(info: GnssInfo, drift: number): boolean {
  if (Math.abs(drift) > GROSS_DRIFT_S) return info.receiverTime !== undefined
  return Math.abs(drift) > DRIFT_THRESHOLD_S && isTimeSourceTrustworthy(info)
}

export type GnssClockDeps = {
  /** Defaults to the wall clock. */
  now?: () => number
  claimFile?: string
  helper?: string
}

export class GnssClock {
  private lastStepAt = 0
  private lastClaimAt = 0
  private claimed = false
  private readonly now: () => number
  private readonly claimFile: string
  private readonly helper: string

  constructor(deps: GnssClockDeps = {}) {
    this.now = deps.now ?? Date.now
    this.claimFile = deps.claimFile ?? CLOCK_CLAIM_FILE
    this.helper = deps.helper ?? SET_TIME_HELPER
  }

  /** Feed every info update. */
  update(info: GnssInfo): void {
    if (info.receiverTime === undefined) {
      this.release()
      return
    }
    // The claim tells the helper to stand down, and only a real lock earns it
    if (isTimeSourceTrustworthy(info)) this.claim()
    else this.release()

    const drift = (info.receiverTime - this.now()) / 1000
    if (!mayStep(info, drift)) return
    if (this.now() - this.lastStepAt < MIN_STEP_INTERVAL_MS) return

    this.lastStepAt = this.now()
    this.step(info.receiverTime, drift)
  }

  /** Release the claim. */
  release(): void {
    if (!this.claimed) return
    this.claimed = false
    try {
      fs.rmSync(this.claimFile, { force: true })
    } catch {}
  }

  private claim(): void {
    const now = this.now()
    if (this.claimed && now - this.lastClaimAt < CLAIM_REFRESH_MS) return
    this.lastClaimAt = now
    try {
      fs.writeFileSync(this.claimFile, `${now}\n`)
      this.claimed = true
    } catch (e) {
      console.warn('[gnssClock] could not claim the clock:', (e as Error).message)
    }
  }

  private step(receiverTime: number, drift: number): void {
    if (!fs.existsSync(this.helper)) {
      console.warn(`[gnssClock] ${this.helper} missing — re-run the installer to enable GPS time`)
      return
    }
    const epoch = Math.round(receiverTime / 1000)
    execFile('sudo', ['-n', this.helper, String(epoch)], (err) => {
      if (err) console.warn('[gnssClock] setting the clock failed:', err.message)
      else console.log(`[gnssClock] system clock stepped by ${drift.toFixed(1)}s from GPS`)
    })
  }
}
