import type { Mock } from 'vitest'
import { TransportArbiter } from '../TransportArbiter'
import type { ArbiterDeps, Candidate, Transport } from '../types'

type DepStubs = {
  wirelessAaEnabled: boolean
  wirelessPhoneInRange: boolean
  active: Transport | null
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

const AA_WIRED: Candidate = { transport: 'aa', mode: 'wired' }
const AA_WIRELESS: Candidate = { transport: 'aa', mode: 'wireless' }
const CP_WIRED: Candidate = { transport: 'cp', mode: 'wired' }

describe('TransportArbiter', () => {
  describe('wired candidates', () => {
    test('a wired AA helper session is offered as wired AA', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.detectedCandidates()).toEqual([AA_WIRED])
    })

    test('a wired CP helper session is offered as wired CarPlay', () => {
      const { arbiter } = makeArbiter({ wiredCpSession: true })
      expect(arbiter.detectedCandidates()).toEqual([CP_WIRED])
    })

    test('wired sessions rank before wireless', () => {
      const { arbiter } = makeArbiter({
        wiredAaSession: true,
        wiredCpSession: true,
        wirelessAaEnabled: true
      })
      expect(arbiter.detectedCandidates()).toEqual([AA_WIRED, CP_WIRED, AA_WIRELESS])
    })

    test('a wired candidate follows the helper session without a debounce', () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)

      stubs.wiredAaSession = false
      expect(arbiter.pickPreferred()).toBeNull()
      expect(arbiter.hasNativeCandidate()).toBe(false)
    })

    test('the snapshot reports a wired phone for either wired session', () => {
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
    test('true when the helper holds a wired AA session', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.hasNativeCandidate()).toBe(true)
    })

    test('true when the helper holds a wired CP session', () => {
      const { arbiter } = makeArbiter({ wiredCpSession: true })
      expect(arbiter.hasNativeCandidate()).toBe(true)
    })

    test('true when wireless is enabled and a phone is in range', () => {
      const { arbiter } = makeArbiter({ wirelessAaEnabled: true, wirelessPhoneInRange: true })
      expect(arbiter.hasNativeCandidate()).toBe(true)
    })

    test('false when wireless is disabled and no wired session exists', () => {
      const { arbiter } = makeArbiter({ wirelessAaEnabled: false })
      expect(arbiter.hasNativeCandidate()).toBe(false)
    })
  })

  describe('candidates and overrides', () => {
    test('a wired CP session is offered as wired CarPlay', () => {
      const { arbiter } = makeArbiter({ wiredCpSession: true })
      expect(arbiter.pickPreferred()).toEqual(CP_WIRED)
    })

    test('wireless is offered during a wired AA session even without a phone in range', () => {
      const { arbiter } = makeArbiter({
        wirelessAaEnabled: true,
        wirelessPhoneInRange: false,
        wiredAaSessionActive: true
      })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRELESS)
    })

    test('an active CP session anchors on wired CarPlay', () => {
      const { arbiter } = makeArbiter({
        active: 'cp',
        wiredCpSessionActive: true,
        wiredCpSession: true
      })
      expect(arbiter.pickPreferred()).toEqual(CP_WIRED)
    })

    test('setOverride forces the candidate and fires onChange', () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSession: true })
      stubs.onChange.mockClear()

      arbiter.setOverride(AA_WIRED)
      expect(arbiter.getOverride()).toEqual(AA_WIRED)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('pickPreferred drops an override that is no longer detected', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      arbiter.setOverride(CP_WIRED) // no wired CP session → not detected

      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
      expect(arbiter.getOverride()).toBeNull()
    })
  })

  describe('pickPreferred', () => {
    test('returns null when nothing is present', () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.pickPreferred()).toBeNull()
    })

    test('returns wired aa when only a wired AA session is present', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('returns wireless aa when only wireless is eligible', () => {
      const { arbiter } = makeArbiter({ wirelessAaEnabled: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRELESS)
    })

    test("'auto' sticks to the active transport", () => {
      const { arbiter } = makeArbiter({
        active: 'cp',
        wiredCpSessionActive: true,
        wiredCpSession: true,
        wiredAaSession: true
      })
      // detected = [AA_WIRED, CP_WIRED]; sticks to the active CP rather than the first
      expect(arbiter.pickPreferred()).toEqual(CP_WIRED)
    })

    test('override beats preference', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true, wirelessAaEnabled: true })
      arbiter.prepareSwitch() // anchor AA_WIRED (pref), cycle to AA_WIRELESS
      expect(arbiter.pickPreferred()).toEqual(AA_WIRELESS)
    })

    test('override is dropped when the chosen candidate disappears', () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSession: true, wirelessAaEnabled: true })
      arbiter.prepareSwitch() // override → AA_WIRELESS

      stubs.wirelessAaEnabled = false // wireless no longer offered
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED) // drops the stale override
      expect(arbiter.getOverride()).toBeNull()
    })
  })

  describe('decideNextStart', () => {
    test('none when nothing is present', () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.decideNextStart()).toEqual({ kind: 'none' })
    })

    test('start with the preferred candidate', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', candidate: AA_WIRED })
    })
  })

  describe('prepareSwitch', () => {
    test('refuses to switch when only one candidate is present', () => {
      const { arbiter } = makeArbiter({ wiredAaSession: true })
      expect(arbiter.prepareSwitch().ok).toBe(false)
    })

    test('cycles wired aa → wireless aa when both are eligible', () => {
      const { arbiter } = makeArbiter({
        active: 'aa',
        wiredAaSessionActive: true,
        wiredAaSession: true,
        wirelessAaEnabled: true
      })
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRELESS })
    })

    test('cycles wireless aa → wired aa while the wired session is still there', () => {
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
    test('reports an active wired CP session with no wireless phone anywhere', () => {
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

    test('reports an active wireless CP session as a detected wireless phone', () => {
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

    test('reflects current presence + preference', () => {
      const { arbiter } = makeArbiter({ active: 'aa', wiredAaSession: true })

      expect(arbiter.getSnapshot()).toEqual({
        active: 'aa',
        targetTransport: 'aa',
        targetMode: 'wireless',
        switchPending: false,
        wiredPhoneDetected: true,
        wirelessPhoneDetected: false,
        wirelessPhoneActive: true,
        wiredPhoneActive: false
      })
    })
  })
})
