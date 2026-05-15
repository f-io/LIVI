import type { Device } from 'usb'
import { TransportArbiter } from '../TransportArbiter'
import type { ArbiterDeps, ConnectionPreference, Transport } from '../types'

type DepStubs = {
  preference: ConnectionPreference
  aaEligible: boolean
  active: Transport | null
  dongleSessionActive: boolean
  wiredAaSessionActive: boolean
  onChange: jest.Mock
  onShouldStop: jest.Mock
  onShouldAutoStart: jest.Mock
}

function makeArbiter(overrides: Partial<DepStubs> = {}) {
  const stubs: DepStubs = {
    preference: 'auto',
    aaEligible: false,
    active: null,
    dongleSessionActive: false,
    wiredAaSessionActive: false,
    onChange: jest.fn(),
    onShouldStop: jest.fn(async () => {}),
    onShouldAutoStart: jest.fn(),
    ...overrides
  }
  const deps: ArbiterDeps = {
    getPreference: () => stubs.preference,
    isAaEligible: () => stubs.aaEligible,
    getActiveTransport: () => stubs.active,
    isDongleSessionActive: () => stubs.dongleSessionActive,
    isWiredAaSessionActive: () => stubs.wiredAaSessionActive,
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
    test('returns null when nothing is present', () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.pickPreferred()).toBeNull()
    })

    test('returns dongle when only dongle is present', () => {
      const { arbiter } = makeArbiter()
      arbiter.markDongleConnected(true)
      expect(arbiter.pickPreferred()).toBe('dongle')
    })

    test('returns aa when only a wired phone is present', () => {
      const { arbiter } = makeArbiter()
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toBe('aa')
    })

    test('returns aa when wireless AA is eligible even without a phone', () => {
      const { arbiter } = makeArbiter({ aaEligible: true })
      arbiter.markDongleConnected(true)
      // preference='auto' falls back to dongle since no active transport yet
      expect(arbiter.pickPreferred()).toBe('dongle')
    })

    test("preference 'dongle' picks dongle when both transports present", () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toBe('dongle')
    })

    test("preference 'native' picks aa when both transports present", () => {
      const { arbiter } = makeArbiter({ preference: 'native' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toBe('aa')
    })

    test("preference 'dongle' falls back to aa when no dongle", () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toBe('aa')
    })

    test("'auto' sticks to the active transport", () => {
      const { arbiter } = makeArbiter({ active: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.pickPreferred()).toBe('dongle')
    })

    test('override beats preference', () => {
      const { arbiter } = makeArbiter({ preference: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.prepareFlip()
      expect(arbiter.pickPreferred()).toBe('aa')
    })

    test('override is dropped when the chosen side disappears', () => {
      const { arbiter } = makeArbiter()
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.prepareFlip() // override = 'aa'

      arbiter.markPhoneConnected(false)
      jest.advanceTimersByTime(1_000)

      expect(arbiter.getOverride()).toBeNull()
      expect(arbiter.pickPreferred()).toBe('dongle')
    })
  })

  describe('decideNextStart', () => {
    test('none when nothing is present', () => {
      const { arbiter } = makeArbiter()
      expect(arbiter.decideNextStart()).toEqual({ kind: 'none' })
    })

    test('start with the preferred transport', () => {
      const { arbiter } = makeArbiter()
      arbiter.markPhoneConnected(true, fakeDevice())
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', transport: 'aa' })
    })

    test("preference='native' defers dongle to give the AOAP probe a chance", () => {
      const { arbiter } = makeArbiter({ preference: 'native' })
      arbiter.markDongleConnected(true)
      const decision = arbiter.decideNextStart()
      expect(decision.kind).toBe('defer')
    })

    test('defer eventually expires and falls through to dongle', () => {
      const t0 = Date.now()
      jest.setSystemTime(t0)
      const { arbiter } = makeArbiter({ preference: 'native' })
      arbiter.markDongleConnected(true)
      expect(arbiter.decideNextStart().kind).toBe('defer')
      jest.setSystemTime(t0 + 3_001)
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', transport: 'dongle' })
    })

    test('explicit override skips the defer', () => {
      const { arbiter } = makeArbiter({ preference: 'native' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      arbiter.prepareFlip() // override → dongle
      expect(arbiter.decideNextStart()).toEqual({ kind: 'start', transport: 'dongle' })
    })

    test('resetNativeProbeDefer re-opens the defer window', () => {
      const t0 = Date.now()
      jest.setSystemTime(t0)
      const { arbiter } = makeArbiter({ preference: 'native' })
      arbiter.markDongleConnected(true)
      arbiter.decideNextStart() // sets deadline at t0+3000

      jest.setSystemTime(t0 + 3_001)
      expect(arbiter.decideNextStart().kind).toBe('start')

      arbiter.resetNativeProbeDefer()
      expect(arbiter.decideNextStart().kind).toBe('defer')
    })
  })

  describe('prepareFlip', () => {
    test('refuses to flip when only one transport is present', () => {
      const { arbiter } = makeArbiter({ active: 'dongle' })
      arbiter.markDongleConnected(true)
      const r = arbiter.prepareFlip()
      expect(r.ok).toBe(false)
    })

    test('flips dongle → aa', () => {
      const { arbiter } = makeArbiter({ active: 'dongle' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      const r = arbiter.prepareFlip()
      expect(r).toEqual({ ok: true, target: 'aa' })
      expect(arbiter.getOverride()).toBe('aa')
    })

    test('flips aa → dongle', () => {
      const { arbiter } = makeArbiter({ active: 'aa' })
      arbiter.markDongleConnected(true)
      arbiter.markPhoneConnected(true, fakeDevice())
      const r = arbiter.prepareFlip()
      expect(r).toEqual({ ok: true, target: 'dongle' })
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
        dongleDetected: true,
        nativeDetected: true,
        preference: 'native'
      })
      expect(stubs.onChange).toHaveBeenCalled()
    })
  })
})
