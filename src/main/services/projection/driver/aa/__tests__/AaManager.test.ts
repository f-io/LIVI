import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

class MockAaSession extends EventEmitter {
  opts: { wired: boolean; wiredBridge: unknown; seed: Record<string, unknown> }
  isWiredMode: Mock
  close = vi.fn(async () => undefined)
  setHevcSupported = vi.fn()
  setVp9Supported = vi.fn()
  setAv1Supported = vi.fn()
  setInitialNightMode = vi.fn()
  setClusterStreamActive = vi.fn()
  constructor(opts: MockAaSession['opts']) {
    super()
    this.opts = opts
    this.isWiredMode = vi.fn(() => this.opts.wired)
    spawned.push(this)
  }
}

class MockUsbAoapBridge extends EventEmitter {
  start = vi.fn(async () => undefined)
  stop = vi.fn(async () => undefined)
  drain = vi.fn(async () => undefined)
  forceReenum = vi.fn(async () => undefined)
}

class MockServer extends EventEmitter {
  handler: (sock: unknown) => void
  listen = vi.fn((_port: number, _host: string, cb?: () => void) => {
    cb?.()
    return this
  })
  close = vi.fn()
  constructor(handler: (sock: unknown) => void) {
    super()
    this.handler = handler
  }
}

class MockSocket extends EventEmitter {
  destroy = vi.fn()
  setNoDelay = vi.fn()
  setTimeout = vi.fn()
}

const spawned: MockAaSession[] = []
const lastServer: { instance: MockServer | null } = { instance: null }
const lastBridge: { instance: MockUsbAoapBridge | null } = { instance: null }
const lastConnectSocket: { instance: MockSocket | null } = { instance: null }

vi.mock('../AaSession', () => ({
  AaSession: vi.fn().mockImplementation(function (opts: MockAaSession['opts']) {
    return new MockAaSession(opts)
  })
}))

vi.mock('../stack/index', () => ({ TCP_PORT: 5277 }))

vi.mock('../stack/transport/UsbAoapBridge', () => ({
  UsbAoapBridge: vi.fn().mockImplementation(function () {
    const b = new MockUsbAoapBridge()
    lastBridge.instance = b
    return b
  })
}))

vi.mock('node:net', () => ({
  createServer: vi.fn((_opts: unknown, handler: (sock: unknown) => void) => {
    const s = new MockServer(handler)
    lastServer.instance = s
    return s
  }),
  createConnection: vi.fn(() => {
    const s = new MockSocket()
    lastConnectSocket.instance = s
    return s
  })
}))

import * as net from 'node:net'
import type { Config } from '@shared/types'
import { AaManager } from '../AaManager'
import { AOAP_LOOPBACK_PORT } from '../stack/aoap/constants'
import { UsbAoapBridge } from '../stack/transport/UsbAoapBridge'

const fakeDevice = (serial = 'S1'): USBDevice =>
  ({ vendorId: 0x18d1, productId: 0x4ee1, serialNumber: serial }) as unknown as USBDevice

function newManager(): { mgr: AaManager; onSpawn: Mock } {
  const onSpawn = vi.fn()
  const mgr = new AaManager({
    getConfig: () => ({}) as Config,
    onWillReenumerate: vi.fn(),
    onSpawn
  })
  return { mgr, onSpawn }
}

beforeEach(() => {
  spawned.length = 0
  lastServer.instance = null
  lastBridge.instance = null
  lastConnectSocket.instance = null
  vi.clearAllMocks()
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  vi.spyOn(console, 'error').mockImplementation(function () {})
})
afterEach(() => vi.restoreAllMocks())

describe('AaManager — wireless listener', () => {
  test('startWireless listens on TCP 5277 and a connection spawns a wireless AaSession', () => {
    const { mgr, onSpawn } = newManager()
    mgr.startWireless()
    expect(lastServer.instance).not.toBeNull()
    expect(lastServer.instance!.listen).toHaveBeenCalledWith(5277, '0.0.0.0', expect.any(Function))

    const sock = new MockSocket()
    lastServer.instance!.handler(sock)
    expect(sock.setNoDelay).toHaveBeenCalled()
    expect(sock.setTimeout).toHaveBeenCalledWith(30_000)
    expect(onSpawn).toHaveBeenCalledTimes(1)
    expect(spawned).toHaveLength(1)
    expect(spawned[0]!.opts.wired).toBe(false)
    expect(spawned[0]!.opts.wiredBridge).toBeNull()
  })

  test('startWireless is idempotent (single server)', () => {
    const { mgr } = newManager()
    mgr.startWireless()
    mgr.startWireless()
    expect(net.createServer as Mock).toHaveBeenCalledTimes(1)
  })

  test('stopWireless closes the server and closes only wireless sessions', () => {
    const { mgr } = newManager()
    mgr.startWireless()
    const server = lastServer.instance!
    server.handler(new MockSocket())
    const wireless = spawned[0]!

    mgr.stopWireless()
    expect(server.close).toHaveBeenCalled()
    expect(wireless.close).toHaveBeenCalled()
  })
})

describe('AaManager — wired bring-up', () => {
  test('brings up a UsbAoapBridge and spawns a wired AaSession on loopback connect', async () => {
    const { mgr, onSpawn } = newManager()
    const ok = await mgr.bringUpWired(fakeDevice())
    expect(ok).toBe(true)
    const bridge = lastBridge.instance!
    expect(bridge.start).toHaveBeenCalledWith(AOAP_LOOPBACK_PORT)

    bridge.emit('ready', { host: '127.0.0.1', port: 5278 })
    expect(net.createConnection as Mock).toHaveBeenCalled()

    const sock = lastConnectSocket.instance!
    sock.emit('connect')
    expect(onSpawn).toHaveBeenCalledTimes(1)
    expect(spawned[0]!.opts.wired).toBe(true)
    expect(spawned[0]!.opts.wiredBridge).toBe(bridge)
  })

  test('idempotent per device: a second bring-up for the same device does not build a 2nd bridge', async () => {
    const { mgr } = newManager()
    await mgr.bringUpWired(fakeDevice('same'))
    const ok = await mgr.bringUpWired(fakeDevice('same'))
    expect(ok).toBe(true)
    expect(UsbAoapBridge as unknown as Mock).toHaveBeenCalledTimes(1)
  })

  test('distinct devices each get their own bridge', async () => {
    const { mgr } = newManager()
    await mgr.bringUpWired(fakeDevice('a'))
    await mgr.bringUpWired(fakeDevice('b'))
    expect(UsbAoapBridge as unknown as Mock).toHaveBeenCalledTimes(2)
  })

  test('bridge.start rejection → returns false and frees the device for a retry', async () => {
    ;(UsbAoapBridge as unknown as Mock).mockImplementationOnce(function () {
      const b = new MockUsbAoapBridge()
      b.start = vi.fn(async () => {
        throw new Error('init failed')
      })
      lastBridge.instance = b
      return b
    })
    const { mgr } = newManager()
    const ok = await mgr.bringUpWired(fakeDevice('retry'))
    expect(ok).toBe(false)
    // The failed key was released, so a fresh attempt builds a new bridge.
    await mgr.bringUpWired(fakeDevice('retry'))
    expect(UsbAoapBridge as unknown as Mock).toHaveBeenCalledTimes(2)
  })

  test('a disconnected wired session frees the device key for re-bring-up', async () => {
    const { mgr } = newManager()
    await mgr.bringUpWired(fakeDevice('cycle'))
    lastBridge.instance!.emit('ready', { host: '127.0.0.1', port: 5278 })
    lastConnectSocket.instance!.emit('connect')
    const session = spawned[0]!

    session.emit('disconnected')
    await mgr.bringUpWired(fakeDevice('cycle'))
    expect(UsbAoapBridge as unknown as Mock).toHaveBeenCalledTimes(2)
  })
})

describe('AaManager — codec/night-mode seed', () => {
  test('live setters forward to every live session', () => {
    const { mgr } = newManager()
    mgr.startWireless()
    lastServer.instance!.handler(new MockSocket())
    const s = spawned[0]!

    mgr.setHevcSupported(true)
    mgr.setVp9Supported(true)
    mgr.setAv1Supported(true)
    mgr.setInitialNightMode(true)
    mgr.setClusterStreamActive(false)

    expect(s.setHevcSupported).toHaveBeenCalledWith(true)
    expect(s.setVp9Supported).toHaveBeenCalledWith(true)
    expect(s.setAv1Supported).toHaveBeenCalledWith(true)
    expect(s.setInitialNightMode).toHaveBeenCalledWith(true)
    expect(s.setClusterStreamActive).toHaveBeenCalledWith(false)
  })

  test('new sessions inherit the current seed', () => {
    const { mgr } = newManager()
    mgr.setHevcSupported(true)
    mgr.setInitialNightMode(true)
    mgr.setClusterStreamActive(false)

    mgr.startWireless()
    lastServer.instance!.handler(new MockSocket())
    const seed = spawned[0]!.opts.seed
    expect(seed.hevcSupported).toBe(true)
    expect(seed.initialNightMode).toBe(true)
    expect(seed.clusterStreamActive).toBe(false)
  })
})
