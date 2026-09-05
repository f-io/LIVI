import {
  type ArbiterDeps,
  type Candidate,
  type ConnectionMode,
  candidateEquals,
  type StartDecision,
  type Transport,
  type TransportSnapshot
} from './types'

const AA_WIRED: Candidate = { transport: 'aa', mode: 'wired' }
const AA_WIRELESS: Candidate = { transport: 'aa', mode: 'wireless' }
const CP_WIRED: Candidate = { transport: 'cp', mode: 'wired' }
const CP_WIRELESS: Candidate = { transport: 'cp', mode: 'wireless' }

export class TransportArbiter {
  private override: Candidate | null = null

  private nativeProbeDeferred = false
  private nativeProbeStartedAt = 0
  private nativeProbeDeadline = 0

  constructor(private readonly deps: ArbiterDeps) {}

  // Queries -----------------------------------------------------------------

  getOverride(): Candidate | null {
    return this.override
  }

  hasNativeCandidate(): boolean {
    if (this.deps.hasWiredAaSession() || this.deps.hasWiredCpSession()) return true
    return this.deps.isWirelessEnabled() && this.deps.isWirelessPhoneInRange()
  }

  detectedCandidates(): Candidate[] {
    const list: Candidate[] = []
    // A wired session the helper announced is a candidate, active or beside.
    if (this.deps.hasWiredAaSession()) list.push(AA_WIRED)
    if (this.deps.hasWiredCpSession()) list.push(CP_WIRED)
    const offerWireless =
      this.deps.isWirelessEnabled() &&
      (this.deps.isWirelessPhoneInRange() || this.deps.isWiredAaSessionActive())
    if (offerWireless) list.push(AA_WIRELESS)
    return list
  }

  private currentCandidate(): Candidate | null {
    const active = this.deps.getActiveTransport()
    if (active === 'aa') return this.deps.isWiredAaSessionActive() ? AA_WIRED : AA_WIRELESS
    if (active === 'cp') return this.deps.isWiredCpSessionActive() ? CP_WIRED : CP_WIRELESS
    return null
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
      wiredPhoneDetected: this.deps.hasWiredAaSession() || this.deps.hasWiredCpSession(),
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
    const anchor = this.currentCandidate() ?? (this.pickPreferred() as Candidate)
    const idx = detected.findIndex((c) => candidateEquals(c, anchor))
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
