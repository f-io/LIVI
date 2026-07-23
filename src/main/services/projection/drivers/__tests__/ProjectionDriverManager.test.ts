import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

const { MockDongleDriver, MockAaManager, MockCpManager, lastManager, lastCpManager } = vi.hoisted(
  () => {
    const { EventEmitter } = require('node:events')

    class MockDongleDriver extends EventEmitter {
      send = vi.fn(async () => true)
      close = vi.fn(async () => undefined)
      initialise = vi.fn(async () => undefined)
      start = vi.fn(async () => undefined)
    }

    const lastManager: { instance: unknown } = { instance: null }
    const lastCpManager: { instance: unknown } = { instance: null }

    class MockAaManager {
      opts: { onSpawn: (s: unknown) => void }
      startWireless = vi.fn()
      stopWireless = vi.fn()
      bringUpWired = vi.fn(async () => true)
      setHevcSupported = vi.fn()
      setVp9Supported = vi.fn()
      setAv1Supported = vi.fn()
      setInitialNightMode = vi.fn()
      setClusterStreamActive = vi.fn()
      constructor(opts: { onSpawn: (s: unknown) => void }) {
        this.opts = opts
        lastManager.instance = this
      }
    }

    class MockCpManager {
      opts: { onSpawn: (s: unknown) => void; onHelperPresence: (p: unknown) => void }
      start = vi.fn()
      close = vi.fn()
      setHevcSupported = vi.fn()
      setVp9Supported = vi.fn()
      setAv1Supported = vi.fn()
      setInitialNightMode = vi.fn()
      setClusterStreamActive = vi.fn()
      constructor(opts: { onSpawn: (s: unknown) => void; onHelperPresence: (p: unknown) => void }) {
        this.opts = opts
        lastCpManager.instance = this
      }
    }

    return { MockDongleDriver, MockAaManager, MockCpManager, lastManager, lastCpManager }
  }
)

vi.mock('../../driver/aa/AaManager', () => ({
  AaManager: vi.fn().mockImplementation(function (opts) {
    return new MockAaManager(opts)
  })
}))

vi.mock('../../driver/cp/CpManager', () => ({
  CpManager: vi.fn().mockImplementation(function (opts) {
    return new MockCpManager(opts)
  })
}))

vi.mock('../../messages', () => ({
  DongleDriver: MockDongleDriver,
  MediaData: class MediaData {},
  NavigationData: class NavigationData {}
}))

import { MediaData } from '../../messages'
import { type DriverManagerDeps, ProjectionDriverManager } from '../ProjectionDriverManager'

type Spies = {
  handlers: Required<DriverManagerDeps['handlers']>
  onAaConnected: Mock
  onAaDisconnected: Mock
  onAaPresence: Mock
  onAaCreated: Mock
  onAaReleased: Mock
  onCpConnected: Mock
  onCpDisconnected: Mock
  onCpPresence: Mock
  onCpHelperPresence: Mock
  onCpCreated: Mock
  onCpReleased: Mock
  onPhoneReenumerate: Mock
}

function buildDeps(over: Partial<DriverManagerDeps> = {}): {
  deps: DriverManagerDeps
  spies: Spies
} {
  const handlers = {
    onMessage: vi.fn(),
    onMetaMessage: vi.fn(),
    onFailure: vi.fn(),
    onTargetedConnect: vi.fn(),
    onVideoCodec: vi.fn(),
    onClusterVideoCodec: vi.fn(),
    onVideoConfig: vi.fn(),
    onClusterVideoConfig: vi.fn()
  }
  const onAaConnected = vi.fn()
  const onAaDisconnected = vi.fn()
  const onAaPresence = vi.fn()
  const onAaCreated = vi.fn()
  const onAaReleased = vi.fn()
  const onCpConnected = vi.fn()
  const onCpDisconnected = vi.fn()
  const onCpPresence = vi.fn()
  const onCpHelperPresence = vi.fn()
  const onCpCreated = vi.fn()
  const onCpReleased = vi.fn()
  const onPhoneReenumerate = vi.fn()
  const deps: DriverManagerDeps = {
    handlers,
    onAaConnected,
    onAaDisconnected,
    onAaPresence,
    onAaCreated,
    onAaReleased,
    getAaConfigSeed: () => ({
      hevcSupported: true,
      vp9Supported: false,
      av1Supported: true,
      initialNightMode: undefined
    }),
    onCpConnected,
    onCpDisconnected,
    onCpPresence,
    onCpHelperPresence,
    onCpCreated,
    onCpReleased,
    getCpConfigSeed: () => ({
      hevcSupported: false,
      vp9Supported: false,
      av1Supported: false,
      initialNightMode: undefined
    }),
    onPhoneReenumerate,
    getConfig: () => ({}) as never,
    ...over
  }
  return {
    deps,
    spies: {
      handlers,
      onAaConnected,
      onAaDisconnected,
      onAaPresence,
      onAaCreated,
      onAaReleased,
      onCpConnected,
      onCpDisconnected,
      onCpPresence,
      onCpHelperPresence,
      onCpCreated,
      onCpReleased,
      onPhoneReenumerate
    }
  }
}

function spawnSession(): EventEmitter {
  const session = new EventEmitter()
  const mgr = lastManager.instance as { opts: { onSpawn: (s: unknown) => void } }
  mgr.opts.onSpawn(session)
  return session
}

function spawnCpSession(): EventEmitter {
  const session = new EventEmitter()
  const mgr = lastCpManager.instance as { opts: { onSpawn: (s: unknown) => void } }
  mgr.opts.onSpawn(session)
  return session
}

describe('ProjectionDriverManager', () => {
  beforeEach(() => {
    lastManager.instance = null
    lastCpManager.instance = null
    vi.clearAllMocks()
  })

  test('starts with a dongle as the active driver and forwards driver events to handlers', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    expect(mgr.getActive()).toBe(mgr.dongle)
    expect(mgr.getAaManager()).toBeNull()

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

  test('ensureAaManager creates the manager once and seeds it from the config seed', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    const m = mgr.ensureAaManager() as unknown as InstanceType<typeof MockAaManager>
    expect(m).toBe(lastManager.instance)
    expect(m.setHevcSupported).toHaveBeenCalledWith(true)
    expect(m.setVp9Supported).toHaveBeenCalledWith(false)
    expect(m.setAv1Supported).toHaveBeenCalledWith(true)
    expect(m.setInitialNightMode).toHaveBeenCalledWith(undefined)

    // Idempotent — a second call returns the same instance without re-constructing.
    expect(mgr.ensureAaManager()).toBe(m)
  })

  test('startAaWireless / stopAaWireless / bringUpAaWired delegate to the manager', async () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    mgr.startAaWireless()
    const m = lastManager.instance as unknown as InstanceType<typeof MockAaManager>
    expect(m.startWireless).toHaveBeenCalled()

    mgr.stopAaWireless()
    expect(m.stopWireless).toHaveBeenCalled()

    const device = {} as USBDevice
    await mgr.bringUpAaWired(device)
    expect(m.bringUpWired).toHaveBeenCalledWith(device)
  })

  test('capability setters forward to the live manager', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const m = lastManager.instance as unknown as InstanceType<typeof MockAaManager>

    mgr.setAaHevcSupported(true)
    mgr.setAaVp9Supported(true)
    mgr.setAaAv1Supported(true)
    mgr.setAaInitialNightMode(true)
    mgr.setAaClusterStreamActive(false)

    expect(m.setHevcSupported).toHaveBeenLastCalledWith(true)
    expect(m.setVp9Supported).toHaveBeenLastCalledWith(true)
    expect(m.setAv1Supported).toHaveBeenLastCalledWith(true)
    expect(m.setInitialNightMode).toHaveBeenLastCalledWith(true)
    expect(m.setClusterStreamActive).toHaveBeenLastCalledWith(false)
  })

  test('AA manager is constructed with the onWillReenumerate forwarder', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const opts = (lastManager.instance as { opts: { onWillReenumerate?: (ms: number) => void } })
      .opts
    opts.onWillReenumerate?.(1234)
    expect(spies.onPhoneReenumerate).toHaveBeenCalledWith(1234)
  })

  test('a spawned session fires onAaCreated and forwards connected/presence/disconnected', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()

    const session = spawnSession()
    expect(spies.onAaCreated).toHaveBeenCalledWith(session)

    session.emit('connected')
    expect(spies.onAaConnected).toHaveBeenCalledWith(session)

    session.emit('device-presence', { kind: 'device', name: 'Pixel' })
    expect(spies.onAaPresence).toHaveBeenCalledWith(session, { kind: 'device', name: 'Pixel' })

    session.emit('disconnected')
    expect(spies.onAaDisconnected).toHaveBeenCalledWith(session)
    expect(spies.onAaReleased).toHaveBeenCalledWith(session)
  })

  test('a spawned session is held until routed; meta messages still flow', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const session = spawnSession()

    // Held (not routed): the dongle still feeds the front end.
    session.emit('message', { type: 2 })
    expect(spies.handlers.onMessage).not.toHaveBeenCalled()

    // Meta listener is attached per-session regardless of routing.
    const media = new MediaData()
    session.emit('message', media)
    expect(spies.handlers.onMetaMessage).toHaveBeenCalledWith(session, media)

    // Once routed, its messages reach the routed handler.
    mgr.route(session as never)
    session.emit('message', { type: 3 })
    expect(spies.handlers.onMessage).toHaveBeenCalledWith({ type: 3 })
  })

  test('a routed session that disconnects re-routes to the dongle and detaches its meta', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const session = spawnSession()
    mgr.route(session as never)

    session.emit('disconnected')
    expect(mgr.getActive()).toBe(mgr.dongle)

    spies.handlers.onMetaMessage.mockClear()
    session.emit('message', new MediaData())
    expect(spies.handlers.onMetaMessage).not.toHaveBeenCalled()

    spies.handlers.onMessage.mockClear()
    mgr.dongle.emit('message', { type: 9 })
    expect(spies.handlers.onMessage).toHaveBeenCalledWith({ type: 9 })
  })

  test('selectFor("dongle") routes the dongle', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    expect(mgr.selectFor('dongle')).toBe(mgr.dongle)
  })

  test('ensureCpManager creates the manager once and seeds it from the CP config seed', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    const m = mgr.ensureCpManager() as unknown as InstanceType<typeof MockCpManager>
    expect(m).toBe(lastCpManager.instance)
    expect(m.setHevcSupported).toHaveBeenCalledWith(false)
    expect(m.setVp9Supported).toHaveBeenCalledWith(false)
    expect(m.setAv1Supported).toHaveBeenCalledWith(false)
    expect(m.setInitialNightMode).toHaveBeenCalledWith(undefined)

    expect(mgr.ensureCpManager()).toBe(m)
  })

  test('startCp / CP capability setters delegate to the CP manager', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    mgr.startCp()
    const m = lastCpManager.instance as unknown as InstanceType<typeof MockCpManager>
    expect(m.start).toHaveBeenCalled()

    mgr.setCpHevcSupported(true)
    mgr.setCpClusterStreamActive(false)
    expect(m.setHevcSupported).toHaveBeenLastCalledWith(true)
    expect(m.setClusterStreamActive).toHaveBeenLastCalledWith(false)
  })

  test('a spawned CP session fires onCpCreated and forwards connected/presence/disconnected', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()

    const session = spawnCpSession()
    expect(spies.onCpCreated).toHaveBeenCalledWith(session)

    session.emit('connected')
    expect(spies.onCpConnected).toHaveBeenCalledWith(session)

    session.emit('device-presence', { kind: 'device', btMac: 'aa:bb' })
    expect(spies.onCpPresence).toHaveBeenCalledWith(session, { kind: 'device', btMac: 'aa:bb' })

    mgr.route(session as never)
    session.emit('disconnected')
    expect(spies.onCpDisconnected).toHaveBeenCalledWith(session)
    expect(spies.onCpReleased).toHaveBeenCalledWith(session)
    expect(mgr.getActive()).toBe(mgr.dongle)
  })

  test('helper presence flows to onCpHelperPresence (registry-level, session-independent)', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()
    const m = lastCpManager.instance as {
      opts: { onHelperPresence: (p: unknown) => void }
    }

    m.opts.onHelperPresence({ kind: 'wifi', wifiMac: 'de:ad', connected: true })
    expect(spies.onCpHelperPresence).toHaveBeenCalledWith({
      kind: 'wifi',
      wifiMac: 'de:ad',
      connected: true
    })
  })
})
