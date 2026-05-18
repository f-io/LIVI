import type { Device } from 'usb'
import {
  type ArbiterDeps,
  type Candidate,
  type ConnectionMode,
  type ConnectionPreference,
  candidateEquals,
  type StartDecision,
  type Transport,
  type TransportSnapshot
} from './types'

const DONGLE_DETACH_DEBOUNCE_MS = 4_000
const PHONE_DETACH_DEBOUNCE_MS = 1_000
const NATIVE_PROBE_POLL_MS = 250
const NATIVE_PROBE_MIN_MS = 500
const NATIVE_PROBE_DEADLINE_MS = 15_000

const DONGLE: Candidate = { transport: 'dongle', mode: 'wired' }
const AA_WIRED: Candidate = { transport: 'aa', mode: 'wired' }
const AA_WIRELESS: Candidate = { transport: 'aa', mode: 'wireless' }

export class TransportArbiter {
  private dongleConnected = false
  private phoneConnected = false
  private phoneDevice: Device | null = null
  private reenumUntil = 0
  private override: Candidate | null = null

  private dongleDetachDebounce: NodeJS.Timeout | null = null
  private phoneDetachDebounce: NodeJS.Timeout | null = null

  private nativeProbeDeferred = false
  private nativeProbeStartedAt = 0
  private nativeProbeDeadline = 0

  constructor(private readonly deps: ArbiterDeps) {}

  // Presence ----------------------------------------------------------------

  markDongleConnected(connected: boolean): void {
    if (connected) {
      if (this.dongleDetachDebounce) {
        clearTimeout(this.dongleDetachDebounce)
        this.dongleDetachDebounce = null
      }
      if (this.dongleConnected) return
      this.dongleConnected = true
      this.deps.onChange()
      return
    }

    if (!this.dongleConnected) return
    if (this.dongleDetachDebounce) return

    // The dongle silently re-enumerates itself whenever it's not in use
    const usingDongle = this.deps.isDongleSessionActive()
    const delay = usingDongle ? 0 : DONGLE_DETACH_DEBOUNCE_MS
    this.dongleDetachDebounce = setTimeout(async () => {
      this.dongleDetachDebounce = null
      this.dongleConnected = false
      console.log('[TransportArbiter] dongle marked disconnected')
      this.clearOverrideIfUndetected()

      if (this.deps.isDongleSessionActive()) {
        try {
          await this.deps.onShouldStop()
        } catch (e) {
          console.warn('[TransportArbiter] stop after dongle unplug threw', e)
        }
      }

      this.deps.onChange()

      if (this.detectedCandidates().length > 0) this.deps.onShouldAutoStart()
    }, delay)
  }

  markPhoneConnected(connected: boolean, device?: Device): void {
    if (connected) {
      if (this.phoneDetachDebounce) {
        clearTimeout(this.phoneDetachDebounce)
        this.phoneDetachDebounce = null
        this.phoneConnected = false
        this.phoneDevice = null
        console.log(
          '[TransportArbiter] wired phone re-attach during detach debounce — committing detach inline'
        )
        this.clearOverrideIfUndetected()
        if (this.deps.isWiredAaSessionActive()) {
          void this.deps
            .onShouldStop()
            .catch((e) => console.warn('[TransportArbiter] stop on phone re-attach threw', e))
        }
      }
      const wasConnected = this.phoneConnected
      this.phoneConnected = true
      this.phoneDevice = device ?? this.phoneDevice
      if (!wasConnected) {
        console.log('[TransportArbiter] wired phone marked connected')
        this.deps.onShouldAutoStart()
      }
      this.deps.onChange()
      return
    }

    if (!this.phoneConnected) return
    if (this.phoneDetachDebounce) return

    this.phoneDetachDebounce = setTimeout(async () => {
      this.phoneDetachDebounce = null
      this.phoneConnected = false
      this.phoneDevice = null
      console.log('[TransportArbiter] wired phone marked disconnected')
      this.clearOverrideIfUndetected()

      if (this.deps.isWiredAaSessionActive()) {
        try {
          await this.deps.onShouldStop()
        } catch (e) {
          console.warn('[TransportArbiter] stop after wired unplug threw', e)
        }
      }

      this.deps.onChange()

      if (this.detectedCandidates().length > 0) this.deps.onShouldAutoStart()
    }, PHONE_DETACH_DEBOUNCE_MS)
  }

  expectPhoneReenumeration(durationMs: number): void {
    this.reenumUntil = Date.now() + durationMs
  }

  isExpectingPhoneReenumeration(): boolean {
    return Date.now() < this.reenumUntil
  }

  // Queries -----------------------------------------------------------------

  isDongleDetected(): boolean {
    return this.dongleConnected
  }

  isPhoneConnected(): boolean {
    return this.phoneConnected
  }

  getPhoneDevice(): Device | null {
    return this.phoneDevice
  }

  getOverride(): Candidate | null {
    return this.override
  }

  hasNativeCandidate(): boolean {
    if (this.phoneConnected) return true
    return this.deps.isWirelessEnabled() && this.deps.isWirelessPhoneInRange()
  }

  detectedCandidates(): Candidate[] {
    const list: Candidate[] = []
    if (this.dongleConnected) list.push(DONGLE)
    if (this.phoneConnected) list.push(AA_WIRED)
    if (this.deps.isWirelessEnabled() && this.deps.isWirelessPhoneInRange()) list.push(AA_WIRELESS)
    return list
  }

  private currentCandidate(): Candidate | null {
    const active = this.deps.getActiveTransport()
    if (active === 'dongle') return DONGLE
    if (active === 'aa') return this.deps.isWiredAaSessionActive() ? AA_WIRED : AA_WIRELESS
    if (active === 'cp') return this.deps.isWiredCpSessionActive() ? AA_WIRED : AA_WIRELESS
    return null
  }

  private clearOverrideIfUndetected(): void {
    if (!this.override) return
    const detected = this.detectedCandidates()
    if (!detected.some((c) => candidateEquals(c, this.override!))) {
      this.override = null
    }
  }

  pickPreferred(): Candidate | null {
    const detected = this.detectedCandidates()
    if (detected.length === 0) return null

    if (this.override) {
      if (detected.some((c) => candidateEquals(c, this.override!))) return this.override
      this.override = null
    }

    const pref = this.deps.getPreference()
    const findByT = (t: Transport): Candidate | undefined => detected.find((c) => c.transport === t)

    if (pref === 'dongle') {
      return findByT('dongle') ?? findByT('aa') ?? detected[0]
    }
    if (pref === 'native') {
      return findByT('aa') ?? findByT('cp') ?? detected[0]
    }

    // 'auto' — sticky to current if still detected
    const current = this.currentCandidate()
    if (current && detected.some((c) => candidateEquals(c, current))) return current

    // Fallback priority: wired AA (direct USB) > dongle > wireless
    const wiredAa = detected.find((c) => c.transport === 'aa' && c.mode === 'wired')
    return wiredAa ?? findByT('dongle') ?? detected[0]
  }

  decideNextStart(): StartDecision {
    const target = this.pickPreferred()
    if (target === null) return { kind: 'none' }

    if (target.transport === 'dongle' && !this.override && this.deps.getPreference() === 'native') {
      const now = Date.now()
      if (!this.nativeProbeDeferred) {
        this.nativeProbeDeferred = true
        this.nativeProbeStartedAt = now
        this.nativeProbeDeadline = now + NATIVE_PROBE_DEADLINE_MS
        console.log('[TransportArbiter] preference=native — probing for native candidate')
      }

      const elapsed = now - this.nativeProbeStartedAt
      const minElapsed = elapsed >= NATIVE_PROBE_MIN_MS
      const wirelessExpected = this.deps.isWirelessEnabled()

      if (now < this.nativeProbeDeadline) {
        if (!minElapsed) {
          return { kind: 'defer', retryMs: NATIVE_PROBE_POLL_MS }
        }
        if (wirelessExpected && !this.deps.isWirelessPhoneInRange()) {
          return { kind: 'defer', retryMs: NATIVE_PROBE_POLL_MS }
        }
      } else if (wirelessExpected && !this.deps.isWirelessPhoneInRange()) {
        console.warn(
          '[TransportArbiter] preference=native — wireless phone never came in range, starting dongle'
        )
      }
    }

    return { kind: 'start', candidate: target }
  }

  resetNativeProbeDefer(): void {
    this.nativeProbeDeferred = false
    this.nativeProbeStartedAt = 0
    this.nativeProbeDeadline = 0
  }

  getSnapshot(): TransportSnapshot {
    const active = this.deps.getActiveTransport()
    const isPhoneActive = active === 'aa' || active === 'cp'
    const wired =
      (active === 'aa' && this.deps.isWiredAaSessionActive()) ||
      (active === 'cp' && this.deps.isWiredCpSessionActive())

    const current = this.currentCandidate()
    const intended = this.override ?? current
    const switchPending =
      this.override !== null && (current === null || !candidateEquals(this.override, current))
    const wirelessActiveNow = isPhoneActive && !wired
    return {
      active,
      targetTransport: intended?.transport ?? null,
      targetMode: intended?.mode ?? null,
      switchPending,
      dongleDetected: this.dongleConnected,
      wiredPhoneDetected: this.phoneConnected,
      wirelessPhoneDetected:
        this.deps.isWirelessEnabled() && (this.deps.isWirelessPhoneInRange() || wirelessActiveNow),
      wiredPhoneActive: isPhoneActive && wired,
      wirelessPhoneActive: wirelessActiveNow,
      preference: this.deps.getPreference()
    }
  }

  // Switch ------------------------------------------------------------------

  prepareSwitch(): { ok: boolean; target: Candidate | null } {
    const detected = this.detectedCandidates()
    if (detected.length < 2) return { ok: false, target: this.currentCandidate() }

    // If no session is running, anchor on the preferred candidate
    const anchor = this.currentCandidate() ?? this.pickPreferred()
    const idx = anchor ? detected.findIndex((c) => candidateEquals(c, anchor)) : -1
    const next = detected[(idx + 1) % detected.length]
    this.override = next
    this.resetNativeProbeDefer()
    this.deps.onChange()
    return { ok: true, target: next }
  }
}

export type {
  Candidate,
  ConnectionMode,
  ConnectionPreference,
  Transport,
  TransportSnapshot
} from './types'
