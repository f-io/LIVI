import type { Device } from 'usb'
import { TransportArbiter } from '../TransportArbiter'
import type { ArbiterDeps, ConnectionPreference, Transport } from '../types'

type DepStubs = {
  preference: ConnectionPreference
  wirelessEnabled: boolean
  wirelessPhoneInRange: boolean
  active: Transport | null
  dongleSessionActive: boolean
  wiredAaSessionActive: boolean
  wiredCpSessionActive: boolean
  onChange: jest.Mock
  onShouldStop: jest.Mock
  onShouldAutoStart: jest.Mock
}

function makeArbiter(overrides: Partial<DepStubs> = {}) {
  const stubs: DepStubs = {
    preference: 'auto',
    wirelessEnabled: false,
    wirelessPhoneInRange: true,
    active: null,
    dongleSessionActive: false,
    wiredAaSessionActive: false,
    wiredCpSessionActive: false,
    onChange: jest.fn(),
    onShouldStop: jest.fn(async () => {}),
    onShouldAutoStart: jest.fn(),
    ...overrides
  }
  const deps: ArbiterDeps = {
    getPreference: () => stubs.preference,
    isWirelessEnabled: () => stubs.wirelessEnabled,
    isWirelessPhoneInRange: () => stubs.wirelessPhoneInRange,
    getActiveTransport: () => stubs.active,
    isDongleSessionActive: () => stubs.dongleSessionActive,
    isWiredAaSessionActive: () => stubs.wiredAaSessionActive,
    isWiredCpSessionActive: () => stubs.wiredCpSessionActive,
    onChange: stubs.onChange,
    onShouldStop: stubs.onShouldStop,
    onShouldAutoStart: stubs.onShouldAutoStart
  }
  return { arbiter: new TransportArbiter(deps), stubs }
}

function fakeDevice(): Device {
  return {
    deviceDescriptor: { idVendor: 0x18d1, idProduct: 0x4ee1 }
  } as unknown as Device
}

describe('TransportArbiter', () => {
  beforeEach(() => jest.useFakeTimers())
  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
  })

  describe('presence — dongle', () => {
    test('attach sets state and fires onChange', () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      expect(arbiter.isDongleDetected()).toBe(true)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
    })

    test('attach is idempotent', () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      arbiter.markDongleConnected(true)
      expect(stubs.onChange).toHaveBeenCalledTimes(1)
    })

    test('detach waits the full debounce when dongle session is not active', () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      stubs.onChange.mockClear()

      arbiter.markDongleConnected(false)
      expect(arbiter.isDongleDetected()).toBe(true)
      expect(stubs.onChange).not.toHaveBeenCalled()

      jest.advanceTimersByTime(3_999)
      expect(arbiter.isDongleDetected()).toBe(true)

      jest.advanceTimersByTime(1)
      expect(arbiter.isDongleDetected()).toBe(false)
      expect(stubs.onChange).toHaveBeenCalled()
    })

    test('detach commits immediately when the dongle owns the active session', () => {
      const { arbiter, stubs } = makeArbiter({ dongleSessionActive: true, active: 'dongle' })
      arbiter.markDongleConnected(true)
      stubs.onChange.mockClear()

      arbiter.markDongleConnected(false)
      jest.advanceTimersByTime(0)
      // setTimeout(_, 0) fires on next tick
      jest.runOnlyPendingTimers()

      expect(arbiter.isDongleDetected()).toBe(false)
      expect(stubs.onShouldStop).toHaveBeenCalledTimes(1)
    })

    test('detach re-attach within the window cancels the debounce', () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(true)
      arbiter.markDongleConnected(false)
      jest.advanceTimersByTime(2_000)
      arbiter.markDongleConnected(true)
      jest.advanceTimersByTime(5_000)

      expect(arbiter.isDongleDetected()).toBe(true)
      expect(stubs.onShouldStop).not.toHaveBeenCalled()
    })

    test('detach without prior attach is a no-op', () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markDongleConnected(false)
      expect(stubs.onChange).not.toHaveBeenCalled()
    })

    test('detach triggers onShouldAutoStart when a phone is present', async () => {
      const { arbiter, stubs } = makeArbiter({
        dongleSessionActive: true,
        active: 'dongle'
      })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      stubs.onShouldAutoStart.mockClear()

      arbiter.markDongleConnected(false)
      jest.runOnlyPendingTimers()
      await Promise.resolve()
      await Promise.resolve()

      expect(stubs.onShouldAutoStart).toHaveBeenCalled()
    })
  })

  describe('presence — phone', () => {
    test('attach sets state, stores device, fires autoStart on the first attach', () => {
      const { arbiter, stubs } = makeArbiter()
      const d = fakeDevice()
      arbiter.markPhoneConnected(true, d)

      expect(arbiter.isPhoneConnected()).toBe(true)
      expect(arbiter.getPhoneDevice()).toBe(d)
      expect(stubs.onShouldAutoStart).toHaveBeenCalledTimes(1)
    })

    test('subsequent attaches do not re-fire autoStart', () => {
      const { arbiter, stubs } = makeArbiter()
      arbiter.markPhoneConnected(true, fakeDevice())
      stubs.onShouldAutoStart.mockClear()

      arbiter.markPhoneConnected(true, fakeDevice())
      expect(stubs.onShouldAutoStart).not.toHaveBeenCalled()
    })

    test('detach waits the 1s debounce', () => {
      const { arbiter } = makeArbiter()
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.markPhoneConnected(false)

      jest.advanceTimersByTime(999)
      expect(arbiter.isPhoneConnected()).toBe(true)
      jest.advanceTimersByTime(1)
      expect(arbiter.isPhoneConnected()).toBe(false)
    })

    test('detach stops the wired AA session if it owns the transport', () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSessionActive: true, active: 'aa' })
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.markPhoneConnected(false)
      jest.advanceTimersByTime(1_000)

      expect(stubs.onShouldStop).toHaveBeenCalledTimes(1)
    })

    test('re-attach during detach debounce commits the detach inline', () => {
      const { arbiter, stubs } = makeArbiter({ wiredAaSessionActive: true, active: 'aa' })
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.markPhoneConnected(false)
      // Re-plug while debounce is still pending
      arbiter.markPhoneConnected(true, fakeDevice())

      expect(stubs.onShouldStop).toHaveBeenCalledTimes(1)
      // wasConnected=false → autoStart fires for the fresh device
      expect(stubs.onShouldAutoStart).toHaveBeenCalledTimes(2)
    })
  })

  describe('re-enumeration window', () => {
    test('isExpectingPhoneReenumeration is time-bounded', () => {
      const { arbiter } = makeArbiter()
      const t0 = Date.now()
      jest.setSystemTime(t0)
      arbiter.expectPhoneReenumeration(500)
      expect(arbiter.isExpectingPhoneReenumeration()).toBe(true)

      jest.setSystemTime(t0 + 499)
      expect(arbiter.isExpectingPhoneReenumeration()).toBe(true)

      jest.setSystemTime(t0 + 600)
      expect(arbiter.isExpectingPhoneReenumeration()).toBe(false)
    })
  })

  describe('pickPreferred', () => {
    const DONGLE = { transport: 'dongle', mode: 'wired' }
    const AA_WIRED = { transport: 'aa', mode: 'wired' }
    const AA_WIRELESS = { transport: 'aa', mode: 'wireless' }

    test('returns null when nothing is present', () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.pickPreferred()).toBeNull()
    })

    test('returns dongle when only dongle is present', () => {
      const { arbiter } = makeArbiter()
      arbiter.markDongleConnected(true)
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test('returns wired aa when only a wired phone is present', () => {
      const { arbiter } = makeArbiter()
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('returns wireless aa when only wireless is eligible', () => {
      const { arbiter } = makeArbiter({ wirelessEnabled: true })
      expect(arbiter.pickPreferred()).toEqual(AA_WIRELESS)
    })

    test("preference='auto' with dongle+wireless prefers dongle when no session yet", () => {
      const { arbiter } = makeArbiter({ wirelessEnabled: true })
      arbiter.markDongleConnected(true)
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test("preference 'dongle' picks dongle when both transports present", () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test("preference 'native' picks aa when both transports present", () => {
      const { arbiter } = makeArbiter({ preference: 'native' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test("preference 'dongle' falls back to aa when no dongle", () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test("'auto' sticks to the active transport", () => {
      const { arbiter } = makeArbiter({ active: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })

    test('override beats preference', () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.prepareSwitch()
      expect(arbiter.pickPreferred()).toEqual(AA_WIRED)
    })

    test('override is dropped when the chosen candidate disappears', () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.prepareSwitch() // anchor=DONGLE (pref), cycles to AA_WIRED

      arbiter.markPhoneConnected(false)
      jest.advanceTimersByTime(1_000)

      expect(arbiter.getOverride()).toBeNull()
      expect(arbiter.pickPreferred()).toEqual(DONGLE)
    })
  })

  describe('decideNextStart', () => {
    const DONGLE = { transport: 'dongle', mode: 'wired' }
    const AA_WIRED = { transport: 'aa', mode: 'wired' }

    test('none when nothing is present', () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.decideNextStart()).toEqual({ kind: 'none' })
    })

    test('start with the preferred candidate', () => {
      const { arbiter } = makeArbiter()
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', candidate: AA_WIRED })
    })

    test("preference='native' defers dongle while wireless is enabled but no phone in range", () => {
      const { arbiter } = makeArbiter({
        preference: 'native',
        wirelessEnabled: true,
        wirelessPhoneInRange: false
      })
      arbiter.markDongleConnected(true)
      const decision = arbiter.decideNextStart()
      expect(decision.kind).toBe('defer')
    })

    test('defer falls through to dongle once wireless phone is in range', () => {
      const t0 = Date.now()
      jest.setSystemTime(t0)
      const { arbiter, stubs } = makeArbiter({
        preference: 'native',
        wirelessEnabled: true,
        wirelessPhoneInRange: false
      })
      arbiter.markDongleConnected(true)
      expect(arbiter.decideNextStart().kind).toBe('defer')
      // Note: once phone is in range, pickPreferred picks AA_WIRELESS, not dongle
      stubs.wirelessPhoneInRange = true
      jest.setSystemTime(t0 + 600)
      const decision = arbiter.decideNextStart()
      expect(decision.kind).toBe('start')
      if (decision.kind === 'start') {
        expect(decision.candidate.transport).toBe('aa')
      }
    })

    test('defer eventually expires after the safety deadline', () => {
      const t0 = Date.now()
      jest.setSystemTime(t0)
      const { arbiter } = makeArbiter({
        preference: 'native',
        wirelessEnabled: true,
        wirelessPhoneInRange: false
      })
      arbiter.markDongleConnected(true)
      expect(arbiter.decideNextStart().kind).toBe('defer')
      jest.setSystemTime(t0 + 15_001)
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', candidate: DONGLE })
    })

    test('explicit override skips the defer', () => {
      const { arbiter } = makeArbiter({
        preference: 'native',
        wirelessEnabled: true,
        wirelessPhoneInRange: false
      })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.prepareSwitch()
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', candidate: DONGLE })
    })

    test('resetNativeProbeDefer re-opens the defer window', () => {
      const t0 = Date.now()
      jest.setSystemTime(t0)
      const { arbiter } = makeArbiter({
        preference: 'native',
        wirelessEnabled: true,
        wirelessPhoneInRange: false
      })
      arbiter.markDongleConnected(true)
      arbiter.decideNextStart()

      jest.setSystemTime(t0 + 15_001)
      expect(arbiter.decideNextStart().kind).toBe('start')

      arbiter.resetNativeProbeDefer()
      expect(arbiter.decideNextStart().kind).toBe('defer')
    })
  })

  describe('prepareSwitch', () => {
    const DONGLE = { transport: 'dongle', mode: 'wired' }
    const AA_WIRED = { transport: 'aa', mode: 'wired' }
    const AA_WIRELESS = { transport: 'aa', mode: 'wireless' }

    test('refuses to switch when only one candidate is present', () => {
      const { arbiter } = makeArbiter({ active: 'dongle' })
      arbiter.markDongleConnected(true)
      const r = arbiter.prepareSwitch()
      expect(r.ok).toBe(false)
    })

    test('switches dongle → wired aa', () => {
      const { arbiter, stubs } = makeArbiter({ active: 'dongle', dongleSessionActive: true })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRED })
      expect(arbiter.getOverride()).toEqual(AA_WIRED)
      // sanity: the underlying snapshot session-active stub is what determines current
      expect(stubs.dongleSessionActive).toBe(true)
    })

    test('switches wired aa → dongle', () => {
      const { arbiter } = makeArbiter({ active: 'aa', wiredAaSessionActive: true })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: DONGLE })
    })

    test('cycles wired aa → wireless aa when both are eligible without dongle', () => {
      const { arbiter } = makeArbiter({
        active: 'aa',
        wiredAaSessionActive: true,
        wirelessEnabled: true
      })
      arbiter.markPhoneConnected(true, fakeDevice())
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRELESS })
    })

    test('cycles wireless aa → wired aa when phone is still plugged', () => {
      const { arbiter } = makeArbiter({
        active: 'aa',
        wiredAaSessionActive: false,
        wirelessEnabled: true
      })
      arbiter.markPhoneConnected(true, fakeDevice())
      const r = arbiter.prepareSwitch()
      expect(r).toEqual({ ok: true, target: AA_WIRED })
    })
  })

  describe('snapshot', () => {
    test('reflects current presence + preference', () => {
      const { arbiter, stubs } = makeArbiter({ preference: 'native', active: 'aa' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())

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
        wiredPhoneActive: false,
        preference: 'native'
      })
      expect(stubs.onChange).toHaveBeenCalled()
    })
  })
})
