import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

type SessionOpts = {
  transport: unknown
  wired: boolean
  usbSerial?: string
  seed: Record<string, unknown>
}

class MockAaSession extends EventEmitter {
  opts: SessionOpts
  isWiredMode: Mock
  close = vi.fn(async () => undefined)
  setHevcSupported = vi.fn()
  setVp9Supported = vi.fn()
  setAv1Supported = vi.fn()
  setInitialNightMode = vi.fn()
  setClusterStreamActive = vi.fn()
  sendSpeedData = vi.fn()
  sendRpmData = vi.fn()
  sendGearData = vi.fn()
  sendNightModeData = vi.fn()
  sendParkingBrakeData = vi.fn()
  sendDrivingStatusData = vi.fn()
  sendLightData = vi.fn()
  sendFuelData = vi.fn()
  sendOdometerData = vi.fn()
  sendEnvironmentData = vi.fn()
  sendGpsLocationData = vi.fn()
  sendVehicleEnergyModel = vi.fn()
  constructor(opts: SessionOpts) {
    super()
    this.opts = opts
    this.isWiredMode = vi.fn(() => this.opts.wired)
    spawned.push(this)
  }
}

class MockLink extends EventEmitter {
  destroy = vi.fn()
  constructor(readonly peer: string) {
    super()
  }
}

type HelperEvent = {
  event: string
  socket?: string
  peer?: string
  transport?: string
  serial?: string
}

/** A helper event stream the test drives by hand. */
class FakeHelper {
  onEvent: ((ev: HelperEvent) => void) | null = null
  onClose: (() => void) | null = null
  closed = vi.fn()
  subscribe = vi.fn((onEvent: (ev: HelperEvent) => void, onClose?: () => void) => {
    this.onEvent = onEvent
    this.onClose = onClose ?? null
    return { close: this.closed }
  })
}

const spawned: MockAaSession[] = []
const links: MockLink[] = []

vi.mock('../AaSession', () => ({
  AaSession: vi.fn().mockImplementation(function (opts: SessionOpts) {
    return new MockAaSession(opts)
  })
}))

vi.mock('../stack/transport/HelperSessionLink', () => ({
  HelperSessionLink: {
    connect: vi.fn(async (_socket: string, peer: string) => {
      const link = new MockLink(peer)
      links.push(link)
      return link
    })
  }
}))

import type { Config } from '@shared/types'
import { AaManager } from '../AaManager'
import { HelperSessionLink } from '../stack/transport/HelperSessionLink'

const connect = HelperSessionLink.connect as unknown as Mock

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve))

function newManager(): { mgr: AaManager; onSpawn: Mock; config: Partial<Config> } {
  const onSpawn = vi.fn()
  const config: Partial<Config> = {}
  const mgr = new AaManager({ getConfig: () => config as Config, onSpawn })
  return { mgr, onSpawn, config }
}

/** A manager whose sessions come from a fake helper. */
function newHelperManager(): ReturnType<typeof newManager> & { helper: FakeHelper } {
  const base = newManager()
  const helper = new FakeHelper()
  base.mgr.attachHelper(helper)
  return { ...base, helper }
}

/** The helper announces a phone it carries over WiFi. */
async function announce(helper: FakeHelper, peer: string, socket = `/tmp/aa-session-${peer}.sock`) {
  helper.onEvent?.({ event: 'aa-session', socket, peer, transport: 'wifi' })
  await flush()
}

/** The helper announces a phone it switched to accessory mode on USB. */
async function announceUsb(
  helper: FakeHelper,
  serial: string,
  socket = `/tmp/aa-session-${serial}.sock`
) {
  helper.onEvent?.({ event: 'aa-session', socket, peer: `usb:${serial}`, transport: 'usb', serial })
  await flush()
}

beforeEach(() => {
  spawned.length = 0
  links.length = 0
  vi.clearAllMocks()
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  vi.spyOn(console, 'error').mockImplementation(function () {})
})
afterEach(() => vi.restoreAllMocks())

describe('AaManager: sessions from the helper', () => {
  test('a wifi announcement without a peer label spawns and supersedes nothing', async () => {
    const { helper } = newHelperManager()
    helper.onEvent?.({ event: 'aa-session', socket: '/tmp/aa-session-x.sock', transport: 'wifi' })
    await flush()
    expect(spawned).toHaveLength(1)
  })

  test('the resubscribe is a no-op once the helper is detached', () => {
    const { mgr } = newHelperManager()
    mgr.detachHelper()
    expect(() => (mgr as unknown as { _openHelperSub: () => void })._openHelperSub()).not.toThrow()
  })

  test('attachHelper subscribes and a wifi announcement spawns a wireless AaSession on its link', async () => {
    const { helper, onSpawn } = newHelperManager()
    expect(helper.subscribe).toHaveBeenCalledTimes(1)

    await announce(helper, '10.10.0.14')
    expect(connect).toHaveBeenCalledWith('/tmp/aa-session-10.10.0.14.sock', '10.10.0.14')
    expect(onSpawn).toHaveBeenCalledTimes(1)
    expect(spawned[0]!.opts.transport).toBe(links[0])
    expect(spawned[0]!.opts.wired).toBe(false)
    expect(spawned[0]!.opts.usbSerial).toBeUndefined()
  })

  test('attachHelper is idempotent (single subscription)', () => {
    const { mgr, helper } = newHelperManager()
    mgr.attachHelper(helper)
    expect(helper.subscribe).toHaveBeenCalledTimes(1)
  })

  test('without a helper there is nothing to attach', () => {
    const { mgr } = newManager()
    expect(() => mgr.attachHelper(undefined)).not.toThrow()
    expect(console.warn).toHaveBeenCalled()
  })

  test('other events and announcements without a socket are ignored', async () => {
    const { helper, onSpawn } = newHelperManager()
    helper.onEvent?.({ event: 'aa-device' })
    helper.onEvent?.({ event: 'aa-session', peer: '10.10.0.14', transport: 'wifi' })
    await flush()
    expect(connect).not.toHaveBeenCalled()
    expect(onSpawn).not.toHaveBeenCalled()
  })

  test('a link that cannot be opened is logged, not thrown', async () => {
    connect.mockImplementationOnce(async () => {
      throw new Error('ECONNREFUSED')
    })
    const { helper, onSpawn } = newHelperManager()
    await announce(helper, '10.10.0.14')
    expect(onSpawn).not.toHaveBeenCalled()
    expect(console.warn).toHaveBeenCalled()
  })

  test('a link arriving after detachHelper is dropped again', async () => {
    const { mgr, helper } = newHelperManager()
    helper.onEvent?.({
      event: 'aa-session',
      socket: '/tmp/x.sock',
      peer: '10.10.0.14',
      transport: 'wifi'
    })
    mgr.detachHelper()
    await flush()
    expect(links[0]!.destroy).toHaveBeenCalled()
    expect(spawned).toHaveLength(0)
  })

  test('the subscription comes back once the helper does', () => {
    vi.useFakeTimers()
    try {
      const { helper } = newHelperManager()
      helper.onClose?.()
      vi.advanceTimersByTime(2000)
      expect(helper.subscribe).toHaveBeenCalledTimes(2)
    } finally {
      vi.useRealTimers()
    }
  })

  test('a closed subscription stays closed after detachHelper', () => {
    vi.useFakeTimers()
    try {
      const { mgr, helper } = newHelperManager()
      mgr.detachHelper()
      helper.onClose?.()
      vi.advanceTimersByTime(2000)
      expect(helper.subscribe).toHaveBeenCalledTimes(1)
    } finally {
      vi.useRealTimers()
    }
  })

  test('a detached helper can be attached again', () => {
    const { mgr, helper } = newHelperManager()
    mgr.detachHelper()
    mgr.attachHelper(helper)
    expect(helper.subscribe).toHaveBeenCalledTimes(2)
  })
})

describe('AaManager: usb sessions from the helper', () => {
  test('a usb announcement spawns a wired AaSession carrying the phone serial', async () => {
    const { helper, onSpawn } = newHelperManager()
    await announceUsb(helper, 'SN-42')
    expect(connect).toHaveBeenCalledWith('/tmp/aa-session-SN-42.sock', 'usb:SN-42')
    expect(onSpawn).toHaveBeenCalledTimes(1)
    expect(spawned[0]!.opts.transport).toBe(links[0])
    expect(spawned[0]!.opts.wired).toBe(true)
    expect(spawned[0]!.opts.usbSerial).toBe('SN-42')
  })

  test('usb sessions take no part in the wireless supersede', async () => {
    const { helper } = newHelperManager()
    const usb = { event: 'aa-session', peer: 'phone', transport: 'usb', serial: 'SN-42' }
    helper.onEvent?.({ ...usb, socket: '/tmp/a.sock' })
    helper.onEvent?.({ ...usb, socket: '/tmp/b.sock' })
    helper.onEvent?.({
      event: 'aa-session',
      socket: '/tmp/c.sock',
      peer: 'phone',
      transport: 'wifi'
    })
    await flush()
    expect(spawned).toHaveLength(3)
    expect(spawned[0]!.close).not.toHaveBeenCalled()
    expect(spawned[1]!.close).not.toHaveBeenCalled()
  })
})

describe('AaManager: stopping', () => {
  test('stopWireless is a no-op when nothing is attached', () => {
    const { mgr } = newManager()
    expect(() => mgr.stopWireless()).not.toThrow()
  })

  test('stopWireless closes the wireless sessions and keeps the subscription', async () => {
    const { mgr, helper } = newHelperManager()
    await announce(helper, '10.10.0.14')
    const wireless = spawned[0]!

    mgr.stopWireless()
    expect(wireless.close).toHaveBeenCalled()
    expect(helper.closed).not.toHaveBeenCalled()
  })

  test('stopWireless leaves usb sessions running', async () => {
    const { mgr, helper } = newHelperManager()
    await announceUsb(helper, 'SN-1')
    await announce(helper, '10.10.0.14')
    const wired = spawned[0]!
    const wireless = spawned[1]!

    mgr.stopWireless()
    expect(wired.close).not.toHaveBeenCalled()
    expect(wireless.close).toHaveBeenCalled()
  })

  test('detachHelper closes the subscription and every session, usb included', async () => {
    const { mgr, helper } = newHelperManager()
    await announceUsb(helper, 'SN-1')
    await announce(helper, '10.10.0.14')

    mgr.detachHelper()
    expect(helper.closed).toHaveBeenCalled()
    expect(spawned[0]!.close).toHaveBeenCalled()
    expect(spawned[1]!.close).toHaveBeenCalled()
  })

  test('close shuts the subscription and the sessions, tolerating throws', async () => {
    const { mgr, helper } = newHelperManager()
    await announce(helper, '10.10.0.14')
    await announceUsb(helper, 'SN-1')
    const wireless = spawned[0]!
    const wired = spawned[1]!
    wireless.close = vi.fn(async () => {
      throw new Error('sess')
    })
    await expect(mgr.close()).resolves.toBeUndefined()
    expect(helper.closed).toHaveBeenCalled()
    expect(wireless.close).toHaveBeenCalled()
    expect(wired.close).toHaveBeenCalled()
  })

  test('close with nothing attached still resolves', async () => {
    const { mgr } = newManager()
    await expect(mgr.close()).resolves.toBeUndefined()
  })

  test('a reconnect supersedes the same-peer session only', async () => {
    const { helper } = newHelperManager()
    await announce(helper, '10.0.0.5', '/tmp/a.sock')
    await announce(helper, '10.0.0.9', '/tmp/b.sock')
    const s1 = spawned[0]!
    const s2 = spawned[1]!
    await announce(helper, '10.0.0.5', '/tmp/c.sock')
    expect(s1.close).toHaveBeenCalled()
    expect(s2.close).not.toHaveBeenCalled()
  })

  test('a disconnected session leaves the fan-out and the peer map', async () => {
    const { mgr, helper } = newHelperManager()
    await announce(helper, '10.0.0.7', '/tmp/a.sock')
    const s = spawned[0]!
    s.emit('disconnected')

    mgr.sendSpeedData(4200)
    expect(s.sendSpeedData).not.toHaveBeenCalled()
    await announce(helper, '10.0.0.7', '/tmp/b.sock')
    expect(s.close).not.toHaveBeenCalled()
  })
})

describe('AaManager: codec/night-mode seed', () => {
  test('live setters forward to every live session', async () => {
    const { mgr, helper } = newHelperManager()
    await announce(helper, '10.0.0.1')
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

  test('new sessions inherit the current seed', async () => {
    const { mgr } = newManager()
    mgr.setHevcSupported(true)
    mgr.setClusterStreamActive(false)

    const helper = new FakeHelper()
    mgr.attachHelper(helper)
    await announce(helper, '10.0.0.1')
    const seed = spawned[0]!.opts.seed
    expect(seed.hevcSupported).toBe(true)
    expect(seed.clusterStreamActive).toBe(false)
  })

  test('a new session inherits the stored night mode', async () => {
    const { mgr } = newManager()
    mgr.setInitialNightMode(true)

    const helper = new FakeHelper()
    mgr.attachHelper(helper)
    await announce(helper, '10.0.0.1')

    expect(spawned[0]!.opts.seed.initialNightMode).toBe(true)
  })

  test('a live night-mode change reaches every session', async () => {
    const { mgr, helper } = newHelperManager()
    await announce(helper, '10.0.0.1')
    await announce(helper, '10.0.0.2')

    mgr.setInitialNightMode(true)

    expect(spawned[0]!.setInitialNightMode).toHaveBeenCalledWith(true)
    expect(spawned[1]!.setInitialNightMode).toHaveBeenCalledWith(true)
  })

  test('telemetry fans out to every connected session', async () => {
    const { mgr, helper } = newHelperManager()
    await announce(helper, '10.0.0.1')
    await announce(helper, '10.0.0.2')

    mgr.sendSpeedData(4200)
    mgr.sendRpmData(3000)
    mgr.sendGearData(4)
    mgr.sendNightModeData(true)
    mgr.sendParkingBrakeData(true)
    mgr.sendDrivingStatusData(1)
    mgr.sendLightData(2, false, 1)
    mgr.sendFuelData(80, 400, false)
    mgr.sendOdometerData(1200, 30)
    mgr.sendEnvironmentData(21000, 101000, 0)
    mgr.sendGpsLocationData({ latDeg: 1, lngDeg: 2 })
    mgr.sendVehicleEnergyModel(60000, 45000, 300000)

    for (const s of spawned) {
      expect(s.sendSpeedData).toHaveBeenCalledWith(4200, undefined, undefined)
      expect(s.sendRpmData).toHaveBeenCalledWith(3000)
      expect(s.sendGearData).toHaveBeenCalledWith(4)
      expect(s.sendNightModeData).toHaveBeenCalledWith(true)
      expect(s.sendParkingBrakeData).toHaveBeenCalledWith(true)
      expect(s.sendDrivingStatusData).toHaveBeenCalledWith(1)
      expect(s.sendLightData).toHaveBeenCalledWith(2, false, 1)
      expect(s.sendFuelData).toHaveBeenCalledWith(80, 400, false)
      expect(s.sendOdometerData).toHaveBeenCalledWith(1200, 30)
      expect(s.sendEnvironmentData).toHaveBeenCalledWith(21000, 101000, 0)
      expect(s.sendGpsLocationData).toHaveBeenCalledWith({ latDeg: 1, lngDeg: 2 })
      expect(s.sendVehicleEnergyModel).toHaveBeenCalledWith(60000, 45000, 300000, undefined)
    }
  })
})
