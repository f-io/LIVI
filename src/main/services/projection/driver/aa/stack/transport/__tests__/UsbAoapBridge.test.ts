import { EventEmitter } from 'node:events'

// ── WebUSB-shaped device mock (usb@3 / node-usb-rs) ────────────────────────────
//
// The bridge talks to a `USBDevice`: async open/close/reset, selectConfiguration,
// claimInterface/releaseInterface, transferIn/transferOut, clearHalt. Endpoints
// live under configuration.interfaces[].alternate.endpoints[] with { type,
// direction, endpointNumber }.

class MockDevice {
  vendorId = 0x18d1
  productId = 0x4ee1

  configuration: USBConfiguration | undefined

  open = jest.fn(async () => undefined)
  close = jest.fn(async () => undefined)
  reset = jest.fn(async () => undefined)
  selectConfiguration = jest.fn(async (value: number) => {
    this.configuration = makeConfig(value)
  })
  claimInterface = jest.fn(async (_n: number) => undefined)
  releaseInterface = jest.fn(async (_n: number) => undefined)
  clearHalt = jest.fn(async (_dir: USBDirection, _ep: number) => undefined)

  // transferIn is gated so the test controls when an IN read resolves. By default
  // it parks forever (a real bulk IN blocks until data or timeout) so the pump
  // loop doesn't busy-spin during the test.
  private _inResolvers: ((r: USBInTransferResult) => void)[] = []
  transferIn = jest.fn(
    (_ep: number, _len: number, _timeoutMs?: number): Promise<USBInTransferResult> =>
      new Promise<USBInTransferResult>((resolve) => {
        this._inResolvers.push(resolve)
      })
  )

  transferOut = jest.fn(
    async (_ep: number, data: BufferSource): Promise<USBOutTransferResult> =>
      ({ status: 'ok', bytesWritten: (data as ArrayBufferView).byteLength }) as USBOutTransferResult
  )

  /** Resolve the oldest pending transferIn with the given bytes (or empty/no-data). */
  resolveIn(data?: Buffer, status: 'ok' | 'stall' = 'ok'): void {
    const resolve = this._inResolvers.shift()
    if (!resolve) return
    resolve({
      status,
      data: data ? new DataView(data.buffer, data.byteOffset, data.byteLength) : undefined
    } as USBInTransferResult)
  }

  constructor(withEndpoints = true) {
    this.configuration = withEndpoints ? makeConfig(1) : makeConfig(1, false)
  }
}

function makeConfig(configurationValue: number, withBulk = true): USBConfiguration {
  const endpoints = withBulk
    ? [
        { endpointNumber: 1, direction: 'in', type: 'bulk', packetSize: 512 },
        { endpointNumber: 2, direction: 'out', type: 'bulk', packetSize: 512 }
      ]
    : []
  return {
    configurationValue,
    configurationName: undefined,
    interfaces: [
      {
        interfaceNumber: 0,
        claimed: false,
        alternate: { alternateSetting: 0, endpoints },
        alternates: []
      }
    ]
  } as unknown as USBConfiguration
}

// ── net mock ───────────────────────────────────────────────────────────────────

class MockServer extends EventEmitter {
  listen = jest.fn((_port: number, _addr: string, cb: () => void) => cb())
  close = jest.fn((cb?: () => void) => cb?.())
}

class MockLoopbackSocket extends EventEmitter {
  setNoDelay = jest.fn()
  write = jest.fn(() => true)
  destroy = jest.fn()
}

const createServer = jest.fn()
jest.mock('net', () => ({
  __esModule: true,
  createServer: (...a: unknown[]) => createServer(...a)
}))

// The bridge imports the `usb` singleton only for hotplug events during the
// non-accessory boot path (waitForAccessoryAttach). Our tests always start from
// accessory mode, so addEventListener/removeEventListener are never exercised —
// stub them so the import resolves.
jest.mock('usb', () => ({
  __esModule: true,
  usb: {
    addEventListener: jest.fn(),
    removeEventListener: jest.fn()
  }
}))

const runAoapHandshakeMock = jest.fn(async () => undefined)
const isAccessoryModeMock = jest.fn(() => true)
jest.mock('../../aoap/handshake', () => ({
  isAccessoryMode: (...a: unknown[]) => isAccessoryModeMock(...a),
  runAoapHandshake: (...a: unknown[]) => runAoapHandshakeMock(...a)
}))

import { UsbAoapBridge } from '../UsbAoapBridge'

type Device = USBDevice

beforeEach(() => {
  createServer.mockReset()
  runAoapHandshakeMock.mockReset()
  isAccessoryModeMock.mockReturnValue(true)
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

/** Wires createServer so the test can grab the connection handler. */
function newBridge(dev: MockDevice = new MockDevice()): {
  dev: MockDevice
  srv: MockServer
  connect: () => (s: MockLoopbackSocket) => void
} {
  const srv = new MockServer()
  let connHandler: ((s: MockLoopbackSocket) => void) | null = null
  createServer.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
    connHandler = h as (s: MockLoopbackSocket) => void
    return srv
  })
  return { dev, srv, connect: () => connHandler! }
}

const flush = (): Promise<void> => new Promise((r) => setImmediate(r))

describe('UsbAoapBridge — start', () => {
  test('refuses double-start', async () => {
    const dev = new MockDevice()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    createServer.mockClear()
    await bridge.start()
    expect(createServer).not.toHaveBeenCalled()
  })

  test('opens the accessory device, selects config, claims iface, emits ready', async () => {
    const dev = new MockDevice()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const ready = jest.fn()
    bridge.on('ready', ready)
    await bridge.start(5278)
    expect(dev.open).toHaveBeenCalled()
    expect(dev.claimInterface).toHaveBeenCalledWith(0)
    expect(ready).toHaveBeenCalledWith(
      expect.objectContaining({ host: expect.any(String), port: 5278 })
    )
  })

  test('open failure surfaces as a thrown error and resets running flag', async () => {
    isAccessoryModeMock.mockReturnValue(true)
    const dev = new MockDevice()
    dev.open.mockRejectedValue(new Error('not found'))
    const onError = jest.fn()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    bridge.on('error', onError)
    await expect(bridge.start()).rejects.toThrow(/Failed to open AOAP accessory/)
    expect(onError).toHaveBeenCalled()
  })

  test('throws when bulk IN/OUT endpoints are missing', async () => {
    const dev = new MockDevice(false) // no bulk endpoints
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    bridge.on('error', () => {}) // swallow the emitted error
    await expect(bridge.start()).rejects.toThrow(/bulk IN\/OUT/)
  })
})

describe('UsbAoapBridge — stop', () => {
  test('idempotent when never started', async () => {
    const dev = new MockDevice()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.stop()).resolves.toBeUndefined()
  })

  test('after a successful start, stop releases, resets, closes and emits "closed"', async () => {
    const dev = new MockDevice()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()

    const closed = jest.fn()
    bridge.on('closed', closed)
    await bridge.stop()
    expect(dev.releaseInterface).toHaveBeenCalledWith(0)
    expect(dev.reset).toHaveBeenCalled()
    expect(dev.close).toHaveBeenCalled()
    expect(closed).toHaveBeenCalled()
  })
})

describe('UsbAoapBridge — drain', () => {
  test('no-op before start', async () => {
    const dev = new MockDevice()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.drain(10)).resolves.toBeUndefined()
  })

  test('resolves within the timeout when outChain is idle', async () => {
    const dev = new MockDevice()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const t0 = Date.now()
    await bridge.drain(100)
    expect(Date.now() - t0).toBeLessThan(500)
  })
})

describe('UsbAoapBridge — forceReenum', () => {
  test('no-op when nothing has been started', async () => {
    const dev = new MockDevice()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.forceReenum()).resolves.toBeUndefined()
  })

  test('after start, forceReenum tears down the loopback server', async () => {
    const { dev, srv } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    await bridge.forceReenum()
    expect(srv.close).toHaveBeenCalled()
  })
})

describe('UsbAoapBridge — loopback server + pump', () => {
  test('client connect → setNoDelay + IN pump starts', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)
    expect(sock.setNoDelay).toHaveBeenCalledWith(true)
    // The pump issues a blocking transferIn on the bulk IN endpoint.
    expect(dev.transferIn).toHaveBeenCalledWith(1, expect.any(Number), expect.any(Number))
  })

  test('second client tears down the first', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const a = new MockLoopbackSocket()
    connect()(a as never)
    const b = new MockLoopbackSocket()
    connect()(b as never)
    expect(a.destroy).toHaveBeenCalled()
  })

  test('USB IN → socket write', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)
    // Resolve the pending bulk IN with a chunk; the pump writes it to the socket.
    dev.resolveIn(Buffer.from([1, 2, 3]))
    await flush()
    expect(sock.write).toHaveBeenCalledWith(Buffer.from([1, 2, 3]))
  })

  test('socket → USB OUT.transferOut', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)
    sock.emit('data', Buffer.from([0xaa]))
    await flush()
    expect(dev.transferOut).toHaveBeenCalledWith(2, Buffer.from([0xaa]))
  })

  test('USB IN disconnect error → emit error + destroy socket', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const onErr = jest.fn()
    bridge.on('error', onErr)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    // First transferIn rejects with a fatal "no device" — pump tears the socket down.
    dev.transferIn.mockImplementationOnce(async () => {
      throw new Error('LIBUSB_ERROR_NO_DEVICE: device gone')
    })
    connect()(sock as never)
    await flush()
    expect(onErr).toHaveBeenCalled()
    expect(sock.destroy).toHaveBeenCalled()
  })

  test('socket close pauses the pump and clears _client', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)
    sock.emit('close')
    expect((bridge as unknown as { _client: unknown })._client).toBeNull()
  })

  test('socket error is forwarded', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const onErr = jest.fn()
    bridge.on('error', onErr)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)
    sock.emit('error', new Error('reset'))
    expect(onErr).toHaveBeenCalled()
  })
})

describe('UsbAoapBridge — accessory open retry', () => {
  test('first 4 opens reject, fifth succeeds', async () => {
    const dev = new MockDevice()
    let attempts = 0
    dev.open.mockImplementation(async () => {
      attempts++
      if (attempts < 5) throw new Error('udev not ready')
    })
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    expect(dev.open).toHaveBeenCalledTimes(5)
  })

  test('5 failed opens throw with the descriptive error', async () => {
    isAccessoryModeMock.mockReturnValue(true)
    const dev = new MockDevice()
    dev.open.mockRejectedValue(new Error('udev not ready'))
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    bridge.on('error', () => {})
    await expect(bridge.start()).rejects.toThrow(/Failed to open AOAP accessory/)
  })

  test('claim retry: first call rejects, second succeeds', async () => {
    const dev = new MockDevice()
    let attempts = 0
    dev.claimInterface.mockImplementation(async () => {
      attempts++
      if (attempts < 2) throw new Error('udev claim race')
    })
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    expect(dev.claimInterface).toHaveBeenCalledTimes(2)
  })

  test('5 failed claims throw a descriptive error', async () => {
    const dev = new MockDevice()
    dev.claimInterface.mockRejectedValue(new Error('busy'))
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    bridge.on('error', () => {})
    await expect(bridge.start()).rejects.toThrow(/Failed to claim AOAP accessory/)
  })
})

describe('UsbAoapBridge — pump edge cases', () => {
  test('USB IN with backpressure awaits drain before the next read', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    sock.write = jest.fn(() => false) // signal backpressure
    connect()(sock as never)

    dev.transferIn.mockClear()
    // Deliver a chunk; sock.write returns false so the pump parks on 'drain'.
    dev.resolveIn(Buffer.from([1]))
    await flush()
    expect(sock.write).toHaveBeenCalled()
    // No further read is issued while backpressured.
    expect(dev.transferIn).not.toHaveBeenCalled()

    // Releasing backpressure lets the pump issue its next read.
    sock.emit('drain')
    await flush()
    expect(dev.transferIn).toHaveBeenCalled()
  })

  test('IN stall clears the halt and keeps pumping', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)

    dev.resolveIn(undefined, 'stall')
    await flush()
    expect(dev.clearHalt).toHaveBeenCalledWith('in', 1)
  })

  test('outChain transferOut error → emit error + destroy socket', async () => {
    const { dev, connect } = newBridge()
    dev.transferOut.mockRejectedValue(new Error('USB stall'))
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const onErr = jest.fn()
    bridge.on('error', onErr)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock as never)
    sock.emit('data', Buffer.from([0xaa]))
    await flush()
    expect(onErr).toHaveBeenCalled()
    expect(sock.destroy).toHaveBeenCalled()
  })
})
