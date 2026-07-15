import {
  type ArbiterDeps,
  type Candidate,
  type ConnectionMode,
  candidateEquals,
  type StartDecision,
  type Transport,
  type TransportSnapshot
} from './types'

type Device = USBDevice

const DONGLE_DETACH_DEBOUNCE_MS = 4_000
const PHONE_DETACH_DEBOUNCE_MS = 1_000

const AA_WIRED: Candidate = { transport: 'aa', mode: 'wired' }
const AA_WIRELESS: Candidate = { transport: 'aa', mode: 'wireless' }
const CP_WIRED: Candidate = { transport: 'cp', mode: 'wired' }
const CP_WIRELESS: Candidate = { transport: 'cp', mode: 'wireless' }
const DONGLE: Candidate = { transport: 'dongle', mode: 'wired' }

const APPLE_VENDOR_ID = 0x05ac

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
        if (this.deps.isWiredAaSessionActive() || this.deps.isWiredCpSessionActive()) {
          this.deps.onWiredPhoneGone()
          this.deps.onShouldAutoStart()
        }
      }
      const wasConnected = this.phoneConnected
      this.phoneConnected = true
      this.phoneDevice = device ?? this.phoneDevice
      if (!wasConnected) {
        console.log('[TransportArbiter] wired phone marked connected')
        if (this.deps.getActiveTransport() !== null) {
          console.log('[TransportArbiter] session active — building wired phone beside it')
          this.deps.onShouldBringUpWiredBeside()
        } else {
          this.deps.onShouldAutoStart()
        }
      }
      this.deps.onChange()
      return
    }

    if (!this.phoneConnected) return
    if (this.phoneDetachDebounce) return

    this.phoneDetachDebounce = setTimeout(() => {
      this.phoneDetachDebounce = null
      this.phoneConnected = false
      this.phoneDevice = null
      console.log('[TransportArbiter] wired phone marked disconnected')
      this.clearOverrideIfUndetected()

      if (this.deps.isWiredAaSessionActive() || this.deps.isWiredCpSessionActive()) {
        this.deps.onWiredPhoneGone()
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
    if (this.phoneConnected) {
      list.push(this.phoneDevice?.vendorId === APPLE_VENDOR_ID ? CP_WIRED : AA_WIRED)
    }
    const offerWireless =
      this.deps.isWirelessEnabled() &&
      (this.deps.isWirelessPhoneInRange() || this.deps.isWiredAaSessionActive())
    if (offerWireless) list.push(AA_WIRELESS)
    return list
  }

  private currentCandidate(): Candidate | null {
    const active = this.deps.getActiveTransport()
    if (active === 'dongle') return DONGLE
    if (active === 'aa') return this.deps.isWiredAaSessionActive() ? AA_WIRED : AA_WIRELESS
    if (active === 'cp') return this.deps.isWiredCpSessionActive() ? CP_WIRED : CP_WIRELESS
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

    const current = this.currentCandidate()
    if (current && detected.some((c) => candidateEquals(c, current))) return current
    return detected[0]
  }

  decideNextStart(): StartDecision {
    const target = this.pickPreferred()
    if (target === null) return { kind: 'none' }
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
        this.deps.isWirelessEnabled() &&
        (this.deps.isWirelessPhoneInRange() ||
          wirelessActiveNow ||
          this.deps.isWiredAaSessionActive()),
      wiredPhoneActive: isPhoneActive && wired,
      wirelessPhoneActive: wirelessActiveNow
    }
  }

  // Switch ------------------------------------------------------------------

  // Force the override to a specific candidate (used by device-list connect)
  setOverride(candidate: Candidate): void {
    this.override = candidate
    this.resetNativeProbeDefer()
    this.deps.onChange()
  }

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
  Transport,
  TransportSnapshot
} from './types'
