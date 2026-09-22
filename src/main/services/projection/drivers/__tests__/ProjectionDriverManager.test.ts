import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

const { MockAaManager, MockCpManager, lastManager, lastCpManager } = vi.hoisted(() => {
  const lastManager: { instance: unknown } = { instance: null }
  const lastCpManager: { instance: unknown } = { instance: null }

  class MockAaManager {
    opts: { onSpawn: (s: unknown) => void }
    attachHelper = vi.fn()
    detachHelper = vi.fn()
    close = vi.fn(async () => undefined)
    stopWireless = vi.fn()
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

  return { MockAaManager, MockCpManager, lastManager, lastCpManager }
})

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
  DuckAudio: class DuckAudio {},
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
  onCpHelperConnect: Mock
  onCpCreated: Mock
  onCpReleased: Mock
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
  const onCpHelperConnect = vi.fn()
  const onCpCreated = vi.fn()
  const onCpReleased = vi.fn()
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
    onCpHelperConnect,
    onCpCreated,
    onCpReleased,
    getCpConfigSeed: () => ({
      hevcSupported: false,
      vp9Supported: false,
      av1Supported: false,
      initialNightMode: undefined
    }),
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
      onCpHelperConnect,
      onCpCreated,
      onCpReleased
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

  test("starts with no active driver and forwards a routed driver's events to handlers", () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    expect(mgr.getActive()).toBeNull()
    expect(mgr.getAaManager()).toBeNull()

    mgr.ensureAaManager()
    const session = spawnSession()
    mgr.route(session as never)

    session.emit('message', { type: 1 })
    session.emit('failure')
    session.emit('targeted-connect-dispatched')
    session.emit('video-codec', 'h264')
    session.emit('cluster-video-codec', 'h265')

    expect(spies.handlers.onMessage).toHaveBeenCalledWith({ type: 1 })
    expect(spies.handlers.onFailure).toHaveBeenCalled()
    expect(spies.handlers.onTargetedConnect).toHaveBeenCalled()
    expect(spies.handlers.onVideoCodec).toHaveBeenCalledWith('h264')
    expect(spies.handlers.onClusterVideoCodec).toHaveBeenCalledWith('h265')
  })

  test('exposes the dongle uploader, idle until a stock dongle is on the bus', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    expect(mgr.getDongleUpload().available).toBe(false)
  })

  test('routing to the already-routed target is a no-op', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const session = spawnSession()
    mgr.route(session as never)
    spies.handlers.onMessage.mockClear()

    mgr.route(session as never) // same target → early return, no re-wiring
    session.emit('message', { type: 7 })
    expect(spies.handlers.onMessage).toHaveBeenCalledTimes(1)
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

  test('attachHelper / detachHelper / stopAaWireless delegate to the manager', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)

    const helper = { subscribe: vi.fn(() => ({ close: vi.fn() })) }
    mgr.attachHelper(helper)
    const m = lastManager.instance as unknown as InstanceType<typeof MockAaManager>
    expect(m.attachHelper).toHaveBeenCalledWith(helper)

    mgr.stopAaWireless()
    expect(m.stopWireless).toHaveBeenCalled()

    mgr.detachHelper()
    expect(m.detachHelper).toHaveBeenCalled()
  })

  test('detachHelper and stopAaWireless are no-ops before a manager exists', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.detachHelper()
    mgr.stopAaWireless()
    expect(mgr.getAaManager()).toBeNull()
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

  test('every supervisor reconnect cycle closes its session again', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const session = spawnSession()

    session.emit('connected')
    session.emit('disconnected')
    expect(spies.onAaDisconnected).toHaveBeenCalledTimes(1)

    // Second cycle: the session closes again and metadata still flows.
    session.emit('connected')
    session.emit('message', new MediaData())
    expect(spies.handlers.onMetaMessage).toHaveBeenCalled()
    session.emit('disconnected')
    expect(spies.onAaConnected).toHaveBeenCalledTimes(2)
    expect(spies.onAaDisconnected).toHaveBeenCalledTimes(2)
  })

  test('a spawned session is held until routed; meta messages still flow', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const session = spawnSession()

    // Held (not routed): its plain messages do not reach the routed handler.
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

  test('a routed session that disconnects re-routes to no driver and detaches its meta', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const session = spawnSession()
    mgr.route(session as never)

    session.emit('disconnected')
    expect(mgr.getActive()).toBeNull()

    spies.handlers.onMetaMessage.mockClear()
    session.emit('message', new MediaData())
    expect(spies.handlers.onMetaMessage).not.toHaveBeenCalled()
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
    expect(mgr.getActive()).toBeNull()
  })

  test('helper connect flows to onCpHelperConnect', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()
    const m = lastCpManager.instance as { opts: { onHelperConnect?: () => void } }

    m.opts.onHelperConnect?.()
    expect(spies.onCpHelperConnect).toHaveBeenCalledTimes(1)
  })

  test('remaining CP capability setters delegate to the CP manager', () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()
    const m = lastCpManager.instance as unknown as InstanceType<typeof MockCpManager>

    mgr.setCpVp9Supported(true)
    mgr.setCpAv1Supported(true)
    mgr.setCpInitialNightMode(false)

    expect(m.setVp9Supported).toHaveBeenLastCalledWith(true)
    expect(m.setAv1Supported).toHaveBeenLastCalledWith(true)
    expect(m.setInitialNightMode).toHaveBeenLastCalledWith(false)
  })

  test('releaseCp without a manager resolves quietly', async () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    await expect(mgr.releaseCp()).resolves.toBeUndefined()
  })

  test('releaseCp closes and drops the manager', async () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()
    const m = lastCpManager.instance as unknown as InstanceType<typeof MockCpManager>

    await mgr.releaseCp()
    expect(m.close).toHaveBeenCalledTimes(1)
    expect(mgr.getCpManager()).toBeNull()
  })

  test('releaseCp swallows a throwing close', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(function () {})
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()
    const m = lastCpManager.instance as unknown as InstanceType<typeof MockCpManager>
    m.close.mockImplementation(function () {
      throw new Error('cp close boom')
    })

    await expect(mgr.releaseCp()).resolves.toBeUndefined()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('releaseAa without a manager resolves quietly', async () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    await expect(mgr.releaseAa()).resolves.toBeUndefined()
  })

  test('releaseAa closes and drops the manager', async () => {
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const m = lastManager.instance as unknown as InstanceType<typeof MockAaManager>

    await mgr.releaseAa()
    expect(m.close).toHaveBeenCalledTimes(1)
    expect(mgr.getAaManager()).toBeNull()
  })

  test('releaseAa swallows a throwing close', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(function () {})
    const { deps } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()
    const m = lastManager.instance as unknown as InstanceType<typeof MockAaManager>
    m.close.mockImplementation(async () => {
      throw new Error('aa close boom')
    })

    await expect(mgr.releaseAa()).resolves.toBeUndefined()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('an unrouted CP session that disconnects leaves no driver routed', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureCpManager()
    const session = spawnCpSession()

    session.emit('disconnected')
    expect(spies.onCpDisconnected).toHaveBeenCalledWith(session)
    expect(mgr.getActive()).toBeNull()
  })

  test('spawning the same session twice attaches its meta listener only once', () => {
    const { deps, spies } = buildDeps()
    const mgr = new ProjectionDriverManager(deps)
    mgr.ensureAaManager()

    const session = spawnSession()
    const mgrInstance = lastManager.instance as { opts: { onSpawn: (s: unknown) => void } }
    mgrInstance.opts.onSpawn(session)

    session.emit('message', new MediaData())
    expect(spies.handlers.onMetaMessage).toHaveBeenCalledTimes(1)

    session.emit('disconnected')
    expect(spies.onAaDisconnected).toHaveBeenCalledTimes(2)
    expect(mgr.getActive()).toBeNull()
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
