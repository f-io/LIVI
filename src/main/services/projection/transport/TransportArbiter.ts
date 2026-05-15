import type { Device } from 'usb'
import type {
  ArbiterDeps,
  ConnectionPreference,
  StartDecision,
  Transport,
  TransportSnapshot
} from './types'

const DONGLE_DETACH_DEBOUNCE_MS = 4_000
const PHONE_DETACH_DEBOUNCE_MS = 1_000
const NATIVE_PROBE_POLL_MS = 250
const NATIVE_PROBE_DEADLINE_MS = 3_000

export class TransportArbiter {
  private dongleConnected = false
  private phoneConnected = false
  private phoneDevice: Device | null = null
  private reenumUntil = 0
  private override: Transport | null = null

  private dongleDetachDebounce: NodeJS.Timeout | null = null
  private phoneDetachDebounce: NodeJS.Timeout | null = null

  private nativeProbeDeferred = false
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
      if (this.override === 'dongle') this.override = null

      if (this.deps.isDongleSessionActive()) {
        try {
          await this.deps.onShouldStop()
        } catch (e) {
          console.warn('[TransportArbiter] stop after dongle unplug threw', e)
        }
      }

      this.deps.onChange()

      if (this.hasNativeCandidate()) this.deps.onShouldAutoStart()
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
        if (this.override === 'aa') this.override = null
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

    this.phoneDetachDebounce = setTimeout(() => {
      this.phoneDetachDebounce = null
      this.phoneConnected = false
      this.phoneDevice = null
      console.log('[TransportArbiter] wired phone marked disconnected')
      if (this.override === 'aa') this.override = null
      if (this.deps.isWiredAaSessionActive()) {
        void this.deps
          .onShouldStop()
          .catch((e) => console.warn('[TransportArbiter] stop after wired unplug threw', e))
      }
      this.deps.onChange()
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

  getOverride(): Transport | null {
    return this.override
  }

  hasNativeCandidate(): boolean {
    if (this.phoneConnected) return true
    return this.deps.isAaEligible()
  }

  pickPreferred(): Transport | null {
    const dongle = this.dongleConnected
    const native = this.hasNativeCandidate()
    if (!dongle && !native) return null

    if (this.override === 'dongle' && dongle) return 'dongle'
    if (this.override === 'aa' && native) return 'aa'
    if (this.override) this.override = null

    const pref = this.deps.getPreference()
    if (pref === 'dongle') return dongle ? 'dongle' : 'aa'
    if (pref === 'native') return native ? 'aa' : 'dongle'

    // 'auto' — sticky to whatever is already active
    const active = this.deps.getActiveTransport()
    if (active === 'dongle' && dongle) return 'dongle'
    if (active === 'aa' && native) return 'aa'
    return dongle ? 'dongle' : 'aa'
  }

  // With preference='native', a synchronously detected dongle can otherwise
  // win the start-race against the async AOAP phone probe.
  decideNextStart(): StartDecision {
    const target = this.pickPreferred()
    if (target === null) return { kind: 'none' }

    if (target === 'dongle' && !this.override && this.deps.getPreference() === 'native') {
      const now = Date.now()
      if (!this.nativeProbeDeferred) {
        this.nativeProbeDeferred = true
        this.nativeProbeDeadline = now + NATIVE_PROBE_DEADLINE_MS
        console.log(
          `[TransportArbiter] preference=native — blocking dongle for up to ${NATIVE_PROBE_DEADLINE_MS}ms`
        )
      }
      if (now < this.nativeProbeDeadline) {
        return { kind: 'defer', retryMs: NATIVE_PROBE_POLL_MS }
      }
      console.log(
        '[TransportArbiter] preference=native — deadline hit, no native candidate, starting dongle'
      )
    }

    return { kind: 'start', transport: target }
  }

  resetNativeProbeDefer(): void {
    this.nativeProbeDeferred = false
    this.nativeProbeDeadline = 0
  }

  getSnapshot(): TransportSnapshot {
    return {
      active: this.deps.getActiveTransport(),
      dongleDetected: this.dongleConnected,
      nativeDetected: this.hasNativeCandidate(),
      preference: this.deps.getPreference()
    }
  }

  // Flip --------------------------------------------------------------------

  prepareFlip(): { ok: boolean; target: Transport | null } {
    const dongle = this.dongleConnected
    const native = this.hasNativeCandidate()
    if (!(dongle && native)) return { ok: false, target: this.deps.getActiveTransport() }

    const current = this.deps.getActiveTransport() ?? this.pickPreferred()
    const target: Transport = current === 'dongle' ? 'aa' : 'dongle'
    this.override = target
    return { ok: true, target }
  }
}

export type { ConnectionPreference, Transport, TransportSnapshot } from './types'
