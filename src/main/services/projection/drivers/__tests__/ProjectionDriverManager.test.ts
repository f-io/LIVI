import { EventEmitter } from 'node:events'

class MockDongleDriver extends EventEmitter {
  send = jest.fn(async () => true)
  close = jest.fn(async () => undefined)
  initialise = jest.fn(async () => undefined)
  start = jest.fn(async () => undefined)
}

class MockAaDriver extends EventEmitter {
  send = jest.fn(async () => true)
  close = jest.fn()
  setHevcSupported = jest.fn()
  setVp9Supported = jest.fn()
  setAv1Supported = jest.fn()
  setInitialNightMode = jest.fn()
  setWiredDevice = jest.fn()
  isWiredMode = jest.fn(() => false)
  start = jest.fn(async () => true)
  restartStack = jest.fn(async () => undefined)
  ctorOpts: unknown
  constructor(opts?: unknown) {
    super()
    this.ctorOpts = opts
  }
}

const lastAaCreated: { instance: MockAaDriver | null } = { instance: null }

jest.mock('../../driver/aa/aaDriver', () => ({
  AaDriver: jest.fn().mockImplementation((opts) => {
    const aa = new MockAaDriver(opts)
    lastAaCreated.instance = aa
    return aa
  })
}))

jest.mock('../../messages', () => ({
  DongleDriver: MockDongleDriver
}))

import { type DriverManagerDeps, ProjectionDriverManager } from '../ProjectionDriverManager'

function buildDeps(over: Partial<DriverManagerDeps> = {}): {
  deps: DriverManagerDeps
  spies: {
    handlers: Required<DriverManagerDeps['handlers']>
    onAaConnected: jest.Mock
    onAaDisconnected: jest.Mock
    onAaCreated: jest.Mock
    onAaReleased: jest.Mock
    onPhoneReenumerate: jest.Mock
  }
} {
  const handlers = {
    onMessage: jest.fn(),
    onFailure: jest.fn(),
    onTargetedConnect: jest.fn(),
    onVideoCodec: jest.fn(),
    onClusterVideoCodec: jest.fn()
  }
  const onAaConnected = jest.fn()
  const onAaDisconnected = jest.fn()
  const onAaCreated = jest.fn()
  const onAaReleased = jest.fn()
  const onPhoneReenumerate = jest.fn()
  const deps: DriverManagerDeps = {
    handlers,
    onAaConnected,
    onAaDisconnected,
    onAaCreated,
    onAaReleased,
    getAaConfigSeed: () => ({
      hevcSupported: true,
      vp9Supported: false,
      av1Supported: true,
      initialNightMode: undefined
    }),
    onPhoneReenumerate,
    ...over
  }
  return {
    deps,
    spies: {
      handlers,
      onAaConnected,
      onAaDisconnected,
      onAaCreated,
      onAaReleased,
      onPhoneReenumerate
    }
  }
}

describe('ProjectionDriverManager', () => {
  beforeEach(() => {
    lastAaCreated.instance = null
  })

  test('starts with a dongle as the active driver and forwards driver events to handlers', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    expect(mgr.getActive()).toBe(mgr.dongle)
    expect(mgr.getAa()).toBeNull()

    mgr.dongle.emit('message', { type: 1 })
    mgr.dongle.emit('failure')
    mgr.dongle.emit('targeted-connect-dispatched')
    mgr.dongle.emit('video-codec', 'h264')
    mgr.dongle.emit('cluster-video-codec', 'h265')

    expect(spies.handlers.onMessage).toHaveBeenCalledWith({ type: 1 })
    expect(spies.handlers.onFailure).toHaveBeenCalled()
    expect(spies.handlers.onTargetedConnect).toHaveBeenCalled()
    expect(spies.handlers.onVideoCodec).toHaveBeenCalledWith('h264')
    expect(spies.handlers.onClusterVideoCodec).toHaveBeenCalledWith('h265')
  })

  test('ensureAa creates an AaDriver and seeds it from the config seed', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    const aa = mgr.ensureAa() as unknown as MockAaDriver
    expect(aa).toBe(lastAaCreated.instance)
    expect(aa.setHevcSupported).toHaveBeenCalledWith(true)
    expect(aa.setVp9Supported).toHaveBeenCalledWith(false)
    expect(aa.setAv1Supported).toHaveBeenCalledWith(true)
    expect(aa.setInitialNightMode).toHaveBeenCalledWith(undefined)
  })

  test('ensureAa swaps listeners: dongle stops emitting, aa starts emitting', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const aa = mgr.ensureAa() as unknown as MockAaDriver

    spies.handlers.onMessage.mockClear()
    mgr.dongle.emit('message', { type: 2 })
    expect(spies.handlers.onMessage).not.toHaveBeenCalled()

    aa.emit('message', { type: 3 })
    expect(spies.handlers.onMessage).toHaveBeenCalledWith({ type: 3 })
  })

  test('AA connected/disconnected forward through deps', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const aa = mgr.ensureAa() as unknown as MockAaDriver

    aa.emit('connected')
    aa.emit('disconnected')
    expect(spies.onAaConnected).toHaveBeenCalledTimes(1)
    expect(spies.onAaDisconnected).toHaveBeenCalledTimes(1)
  })

  test('ensureAa fires onAaCreated exactly once per session', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    mgr.ensureAa()
    mgr.ensureAa()
    expect(spies.onAaCreated).toHaveBeenCalledTimes(1)
  })

  test('selectFor("aa") returns the AA driver', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const sel = mgr.selectFor('aa')
    expect(sel).toBe(mgr.getAa())
  })

  test('selectFor("dongle") releases an existing AA driver and returns the dongle', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const aa = mgr.ensureAa() as unknown as MockAaDriver

    const sel = mgr.selectFor('dongle')
    expect(sel).toBe(mgr.dongle)
    expect(mgr.getAa()).toBeNull()
    expect(aa.close).toHaveBeenCalled()
    expect(spies.onAaReleased).toHaveBeenCalledTimes(1)
  })

  test('releaseAa detaches listeners from AA and re-attaches to dongle', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const aa = mgr.ensureAa() as unknown as MockAaDriver

    mgr.releaseAa()

    spies.handlers.onMessage.mockClear()
    aa.emit('message', { type: 4 })
    expect(spies.handlers.onMessage).not.toHaveBeenCalled()

    mgr.dongle.emit('message', { type: 5 })
    expect(spies.handlers.onMessage).toHaveBeenCalledWith({ type: 5 })
  })

  test('releaseAa is a no-op when no AA driver is set', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.releaseAa()
    expect(spies.onAaReleased).not.toHaveBeenCalled()
  })

  test('AA close() throwing is swallowed and AA is still cleared', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const aa = mgr.ensureAa() as unknown as MockAaDriver
    aa.close.mockImplementation(() => {
      throw new Error('boom')
    })

    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    expect(() => mgr.releaseAa()).not.toThrow()
    expect(mgr.getAa()).toBeNull()
    warn.mockRestore()
  })

  test('AA driver is constructed with the onWillReenumerate forwarder', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    const aa = mgr.ensureAa() as unknown as MockAaDriver
    const opts = aa.ctorOpts as { onWillReenumerate?: (ms: number) => void }
    opts.onWillReenumerate?.(1234)
    expect(spies.onPhoneReenumerate).toHaveBeenCalledWith(1234)
  })
})
