import type { Mock } from 'vitest'
import { TransportArbiter } from '../TransportArbiter'
import type { ArbiterDeps, Candidate, Transport } from '../types'

type DepStubs = {
  wirelessAaEnabled: boolean
  wirelessPhoneInRange: boolean
  active: Transport | null
  dongleSessionActive: boolean
  wiredAaSessionActive: boolean
  wiredCpSessionActive: boolean
  wiredAaSession: boolean
  wiredCpSession: boolean
  onChange: Mock
  onShouldStop: Mock
  onShouldAutoStart: Mock
}

// Wired phones are helper sessions, the two wired stubs stand in for the session manager
function makeArbiter(overrides: Partial<DepStubs> = {}) {
  const stubs: DepStubs = {
    wirelessAaEnabled: false,
    wirelessPhoneInRange: true,
    active: null,
    dongleSessionActive: false,
    wiredAaSessionActive: false,
    wiredCpSessionActive: false,
    wiredAaSession: false,
    wiredCpSession: false,
    onChange: vi.fn(),
    onShouldStop: vi.fn(async () => {}),
    onShouldAutoStart: vi.fn(),
    ...overrides
  }
  const deps: ArbiterDeps = {
    isWirelessEnabled: () => stubs.wirelessAaEnabled,
    isWirelessPhoneInRange: () => stubs.wirelessPhoneInRange,
    getActiveTransport: () => stubs.active,
    isDongleSessionActive: () => stubs.dongleSessionActive,
    isWiredAaSessionActive: () => stubs.wiredAaSessionActive,
    isWiredCpSessionActive: () => stubs.wiredCpSessionActive,
    hasWiredAaSession: () => stubs.wiredAaSession,
    hasWiredCpSession: () => stubs.wiredCpSession,
    onChange: stubs.onChange,
    onShouldStop: stubs.onShouldStop,
    onShouldAutoStart: stubs.onShouldAutoStart
  }
  return { arbiter: new TransportArbiter(deps), stubs }
}

const DONGLE: Candidate = { transport: 'dongle', mode: 'wired' }
const AA_WIRED: Candidate = { transport: 'aa', mode: 'wired' }
const AA_WIRELESS: Candidate = { transport: 'aa', mode: 'wireless' }
const CP_WIRED: Candidate = { transport: 'cp', mode: 'wired' }

describe('TransportArbiter', () => {
  beforeEach(async () => vi.useFakeTimers())
  afterEach(async () => {
    vi.runOnlyPendingTimers()
    vi.useRealTimers()
  })

  describe('dongle presence', () => {
    test('attach sets state and fires onChange', async () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      expect(arbiter.isDongleDetected()).toBe(true)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
    })

    test('attach is idempotent', async () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      arbiter.markDongleConnected(true)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
    })

    test('detach waits the full debounce when dongle session is not active', async () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      stubs.onChange.mockClear()

      arbiter.markDongleConnected(false)
      expect(arbiter.isDongleDetected()).toBe(true)
      expect(stubs.onChange).not.toHaveBeenCalled()

      vi.advanceTimersByTime(3_999)
      expect(arbiter.isDongleDetected()).toBe(true)

      vi.advanceTimersByTime(1)
      expect(arbiter.isDongleDetected()).toBe(false)
      expect(stubs.onChange).toHaveBeenCalled()
    })

    test('detach commits immediately when the dongle owns the active session', async () => {
      const { arbiter, stubs } = makeArbiter({ dongleSessionActive: true, active: 'dongle' })
      arbiter.markDongleConnected(true)
      stubs.onChange.mockClear()

      arbiter.markDongleConnected(false)
      vi.advanceTimersByTime(0)
      // setTimeout(_, 0) fires on next tick
      vi.runOnlyPendingTimers()

      expect(arbiter.isDongleDetected()).toBe(false)
      expect(stubs.onShouldStop).toHaveBeenCalledTimes(1)
    })

    test('detach re-attach within the window cancels the debounce', async () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      arbiter.markDongleConnected(false)
      vi.advanceTimersByTime(2_000)
      arbiter.markDongleConnected(true)
      vi.advanceTimersByTime(5_000)

      expect(arbiter.isDongleDetected()).toBe(true)
      expect(stubs.onShouldStop).not.toHaveBeenCalled()
    })

    test('detach without prior attach is a no-op', async () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(false)
      expect(stubs.onChange).not.toHaveBeenCalled()
    })

    test('detach triggers onShouldAutoStart when a wired AA session is present', async () => {
      const { arbiter, stubs } = makeArbiter({
        dongleSessionActive: true,
        active: 'dongle',
        wiredAaSession: true
      })
      arbiter.markDongleConnected(true)
      stubs.onShouldAutoStart.mockClear()

      arbiter.markDongleConnected(false)
      vi.runOnlyPendingTimers()
      await Promise.resolve()
      await Promise.resolve()

      expect(stubs.onShouldAutoStart).toHaveBeenCalled()
    })
  })

  describe('debounce guards', () => {
    test('a second dongle detach while the debounce is pending is a no-op', async () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      stubs.onChange.mockClear()

      arbiter.markDongleConnected(false)
      arbiter.markDongleConnected(false)
      vi.advanceTimersByTime(4_000)

      expect(arbiter.isDongleDetected()).toBe(false)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
    })

    test('a throwing stop after dongle unplug is contained', async () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(function () {})
      const { arbiter, stubs } = makeArbiter({ dongleSessionActive: true, active: 'dongle' })
      stubs.onShouldStop.mockRejectedValue(new Error('stop boom'))
      arbiter.markDongleConnected(true)

      arbiter.markDongleConnected(false)
      vi.runOnlyPendingTimers()
      await Promise.resolve()
      await Promise.resolve()

      expect(arbiter.isDongleDetected()).toBe(false)
      expect(warn).toHaveBeenCalled()
      warn.mockRestore()
    })
  })

  describe('wired candidates', () => {
    test('a wired AA helper session is offered as wired AA', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.detectedCandidates()).toEqual([AA_WIRED])
    })

    test('a wired CP helper session is offered as wired CarPlay', async () => {
      const { arbiter } = makeArbiter({ wiredCpSession: true })
      expect(arbiter.detectedCandidates()).toEqual([CP_WIRED])
    })

    test('wired sessions rank before wireless and the dongle', async () => {
      const { arbiter } = makeArbiter({
        wiredAaSession: true,
        wiredCpSession: true,
        wirelessAaEnabled: true
      })
      arbiter.markDongleConnected(true)
      expect(arbiter.detectedCandidates()).toEqual([AA_WIRED, CP_WIRED, AA_WIRELESS, DONGLE])
    })

    test('a wired candidate follows the helper session without a debounce', async () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)

      stubs.wiredAaSession = false
      expect(arbiter.pickPreferred()).toBeNull()
      expect(arbiter.hasNativeCandidate()).toBe(false)
    })

    test('the snapshot reports a wired phone for either wired session', async () => {
      expect(makeArbiter().arbiter.getSnapshot().wiredPhoneDetected).toBe(false)
      expect(makeArbiter({ wiredAaSession: true }).arbiter.getSnapshot().wiredPhoneDetected).toBe(
        true
      )
      expect(makeArbiter({ wiredCpSession: true }).arbiter.getSnapshot().wiredPhoneDetected).toBe(
        true
      )
    })
  })

  describe('hasNativeCandidate', () => {
    test('true when the helper holds a wired AA session', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.hasNativeCandidate()).toBe(true)
    })

    test('true when the helper holds a wired CP session', async () => {
      const { arbiter } = makeArbiter({ wiredCpSession: true })
      expect(arbiter.hasNativeCandidate()).toBe(true)
    })

    test('true when wireless is enabled and a phone is in range', async () => {
      const { arbiter } = makeArbiter({ wirelessAaEnabled: true, wirelessPhoneInRange: true })
      expect(arbiter.hasNativeCandidate()).toBe(true)
    })

    test('false when wireless is disabled and no wired session exists', async () => {
      const { arbiter } = makeArbiter({ wirelessAaEnabled: false })
      expect(arbiter.hasNativeCandidate()).toBe(false)
    })
  })

  describe('candidates and overrides', () => {
    test('a wired CP session is offered as wired CarPlay', async () => {
      const { arbiter } = makeArbiter({ wiredCpSession: true })
      expect(arbiter.pickPreferred()).toEqual(CP_WIRED)
    })

    test('wireless is offered during a wired AA session even without a phone in range', async () => {
      const { arbiter } = makeArbiter({
        wirelessAaEnabled: true,
        wirelessPhoneInRange: false,
        wiredAaSessionActive: true
      })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRELESS)
    })

    test('an active CP session anchors on wired or wireless CarPlay', async () => {
      const wired = makeArbiter({ active: 'cp', wiredCpSessionActive: true, wiredCpSession: true })
      wired.arbiter.markDongleConnected(true)
      expect(wired.arbiter.pickPreferred()).toEqual(CP_WIRED)

      const wireless = makeArbiter({ active: 'cp', wiredCpSessionActive: false })
      wireless.arbiter.markDongleConnected(true)
      expect(wireless.arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test('setOverride forces the candidate and fires onChange', async () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSession: true })
      stubs.onChange.mockClear()

      arbiter.setOverride(AA_WIRED)
      expect(arbiter.getOverride()).toEqual(AA_WIRED)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('pickPreferred drops an override that is no longer detected', async () => {
      const { arbiter } = makeArbiter()
      arbiter.markDongleConnected(true)
      arbiter.setOverride(AA_WIRED)

      expect(arbiter.pickPreferred()).toEqual(DONGLE)
      expect(arbiter.getOverride()).toBeNull()
    })

    test('a detach commit keeps an override that is still detected', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      arbiter.markDongleConnected(true)
      arbiter.setOverride(AA_WIRED)

      arbiter.markDongleConnected(false)
      vi.advanceTimersByTime(4_000)

      expect(arbiter.getOverride()).toEqual(AA_WIRED)
    })
  })

  describe('pickPreferred', () => {
    test('returns null when nothing is present', async () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.pickPreferred()).toBeNull()
    })

    test('returns dongle when only dongle is present', async () => {
      const { arbiter } = makeArbiter()
      arbiter.markDongleConnected(true)
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test('returns wired aa when only a wired AA session is present', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('returns wireless aa when only wireless is eligible', async () => {
      const { arbiter } = makeArbiter({ wirelessAaEnabled: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRELESS)
    })

    test("'auto' sticks to the active transport", () => {
      const { arbiter } = makeArbiter({ active: 'dongle', wiredAaSession: true })
      arbiter.markDongleConnected(true)
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test('the wired AA session outranks the dongle', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      arbiter.markDongleConnected(true)
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('override beats preference', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      arbiter.markDongleConnected(true)
      arbiter.prepareSwitch()
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test('override is dropped when the chosen candidate disappears', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      arbiter.markDongleConnected(true)
      arbiter.prepareSwitch() // anchor=AA_WIRED (pref), cycles to DONGLE

      arbiter.markDongleConnected(false)
      vi.advanceTimersByTime(5_000)

      expect(arbiter.getOverride()).toBeNull()
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })
  })

  describe('decideNextStart', () => {
    test('none when nothing is present', async () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.decideNextStart()).toEqual({ kind: 'none' })
    })

    test('start with the preferred candidate', async () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', candidate: AA_WIRED })
    })
  })

  describe('prepareSwitch', () => {
    test('refuses to switch when only one candidate is present', async () => {
      const { arbiter } = makeArbiter({ active: 'dongle' })
      arbiter.markDongleConnected(true)
      const r = arbiter.prepareSwitch()
      expect(r.ok).toBe(false)
    })

    test('switches dongle → wired aa', async () => {
      const { arbiter, stubs } = makeArbiter({
        active: 'dongle',
        dongleSessionActive: true,
        wiredAaSession: true
      })
      arbiter.markDongleConnected(true)
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRED })
      expect(arbiter.getOverride()).toEqual(AA_WIRED)
      // sanity: the underlying snapshot session-active stub is what determines current
      expect(stubs.dongleSessionActive).toBe(true)
    })

    test('switches wired aa → dongle', async () => {
      const { arbiter } = makeArbiter({
        active: 'aa',
        wiredAaSessionActive: true,
        wiredAaSession: true
      })
      arbiter.markDongleConnected(true)
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: DONGLE })
    })

    test('cycles wired aa → wireless aa when both are eligible without dongle', async () => {
      const { arbiter } = makeArbiter({
        active: 'aa',
        wiredAaSessionActive: true,
        wiredAaSession: true,
        wirelessAaEnabled: true
      })
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRELESS })
    })

    test('cycles wireless aa → wired aa while the wired session is still there', async () => {
      const { arbiter } = makeArbiter({
        active: 'aa',
        wiredAaSessionActive: false,
        wiredAaSession: true,
        wirelessAaEnabled: true
      })
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRED })
    })
  })

  describe('snapshot', () => {
    test('reports an active wired CP session with no wireless phone anywhere', async () => {
      const { arbiter } = makeArbiter({
        active: 'cp',
        wiredCpSessionActive: true,
        wiredCpSession: true,
        wirelessAaEnabled: true,
        wirelessPhoneInRange: false
      })

      const snap = arbiter.getSnapshot()
      expect(snap.active).toBe('cp')
      expect(snap.targetTransport).toBe('cp')
      expect(snap.targetMode).toBe('wired')
      expect(snap.wiredPhoneDetected).toBe(true)
      expect(snap.wiredPhoneActive).toBe(true)
      expect(snap.wirelessPhoneActive).toBe(false)
      expect(snap.wirelessPhoneDetected).toBe(false)
    })

    test('reports an active wireless CP session as a detected wireless phone', async () => {
      const { arbiter } = makeArbiter({
        active: 'cp',
        wiredCpSessionActive: false,
        wirelessAaEnabled: true,
        wirelessPhoneInRange: false
      })

      const snap = arbiter.getSnapshot()
      expect(snap.wiredPhoneActive).toBe(false)
      expect(snap.wirelessPhoneActive).toBe(true)
      expect(snap.wirelessPhoneDetected).toBe(true)
    })

    test('reflects current presence + preference', async () => {
      const { arbiter, stubs } = makeArbiter({ active: 'aa', wiredAaSession: true })
      arbiter.markDongleConnected(true)

      const snap = arbiter.getSnapshot()
      expect(snap).toEqual({
        active: 'aa',
        targetTransport: 'aa',
        targetMode: 'wireless',
        switchPending: false,
        dongleDetected: true,
        wiredPhoneDetected: true,
        wirelessPhoneDetected: false,
        wirelessPhoneActive: true,
        wiredPhoneActive: false
      })
      expect(stubs.onChange).toHaveBeenCalled()
    })
  })
})
