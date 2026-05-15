import { EventEmitter } from 'node:events'

class MockEndpoint extends EventEmitter {
  pollActive = false
  transferType = 0x02
  address = 0
  startPoll = jest.fn()
  stopPoll = jest.fn((cb?: () => void) => cb?.())
  transfer = jest.fn((_buf: Buffer, cb: (err?: Error) => void) => cb())
}

class MockInterface {
  endpoints: MockEndpoint[] = []
  claim = jest.fn()
  release = jest.fn((_reset: boolean, cb?: (err?: Error) => void) => cb?.())
}

class MockDevice {
  deviceDescriptor = { idVendor: 0x18d1, idProduct: 0x4ee1 }
  open = jest.fn()
  close = jest.fn()
  interface = jest.fn(() => null as MockInterface | null)
  controlTransfer = jest.fn(
    (
      _bm: number,
      _br: number,
      _wv: number,
      _wi: number,
      _data: Buffer | number,
      cb: (err: Error | null, data?: Buffer) => void
    ) => {
      process.nextTick(() => cb(null))
    }
  )
}

class MockServer extends EventEmitter {
  listen = jest.fn((_port: number, _addr: string, cb: () => void) => cb())
  close = jest.fn((cb?: () => void) => cb?.())
}

const createServer = jest.fn()
jest.mock('net', () => ({
  __esModule: true,
  createServer: (...a: unknown[]) => createServer(...a)
}))

jest.mock('usb', () => ({
  __esModule: true,
  usb: {
    on: jest.fn(),
    off: jest.fn(),
    addListener: jest.fn(),
    removeListener: jest.fn()
  }
}))

const runAoapHandshakeMock = jest.fn(async () => undefined)
const isAccessoryModeMock = jest.fn(() => true)
jest.mock('../../aoap/handshake', () => ({
  isAccessoryMode: (...a: unknown[]) => isAccessoryModeMock(...a),
  runAoapHandshake: (...a: unknown[]) => runAoapHandshakeMock(...a)
}))

import type { Device } from 'usb'
import { UsbAoapBridge } from '../UsbAoapBridge'

beforeEach(() => {
  createServer.mockReset()
  runAoapHandshakeMock.mockReset()
  isAccessoryModeMock.mockReturnValue(true)
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

function deviceWithInterface(): {
  dev: MockDevice
  iface: MockInterface
  inEp: MockEndpoint
  outEp: MockEndpoint
} {
  const dev = new MockDevice()
  const iface = new MockInterface()
  const inEp = new MockEndpoint()
  inEp.transferType = 0x02
  inEp.address = 0x81 // bulk IN
  const outEp = new MockEndpoint()
  outEp.transferType = 0x02
  outEp.address = 0x02 // bulk OUT
  iface.endpoints = [inEp, outEp]
  dev.interface.mockReturnValue(iface)
  return { dev, iface, inEp, outEp }
}

describe('UsbAoapBridge — start', () => {
  test('refuses double-start', async () => {
    const { dev } = deviceWithInterface()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    createServer.mockClear()
    await bridge.start()
    expect(createServer).not.toHaveBeenCalled()
  })

  test('emits ready when loopback server listens', async () => {
    const { dev } = deviceWithInterface()
    const srv = new MockServer()
    createServer.mockImplementationOnce(() => srv)
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const ready = jest.fn()
    bridge.on('ready', ready)
    await bridge.start(5278)
    // listen() invokes the callback synchronously, which emits ready
    expect(ready).toHaveBeenCalledWith(
      expect.objectContaining({ host: expect.any(String), port: 5278 })
    )
  })

  test('open failure surfaces as a thrown error and resets running flag', async () => {
    isAccessoryModeMock.mockReturnValue(true)
    const dev = new MockDevice()
    dev.open.mockImplementation(() => {
      throw new Error('not found')
    })
    const onError = jest.fn()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    bridge.on('error', onError)
    await expect(bridge.start()).rejects.toThrow(/Failed to open AOAP accessory/)
    expect(onError).toHaveBeenCalled()
  })

  test('throws when interface 0 is missing', async () => {
    const dev = new MockDevice()
    dev.interface.mockReturnValue(null)
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.start()).rejects.toThrow(/interface 0 missing/)
  })

  test('throws when bulk IN or OUT endpoint is missing', async () => {
    const dev = new MockDevice()
    const iface = new MockInterface()
    iface.endpoints = [
      Object.assign(new MockEndpoint(), { transferType: 0x02, address: 0x81 }) // only IN
    ]
    dev.interface.mockReturnValue(iface)
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.start()).rejects.toThrow(/bulk IN\/OUT/)
  })
})

describe('UsbAoapBridge — stop', () => {
  test('idempotent when never started', async () => {
    const dev = new MockDevice()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.stop()).resolves.toBeUndefined()
  })

  test('after a successful start, stop tears everything down and emits "closed"', async () => {
    const { dev, iface, inEp } = deviceWithInterface()
    const srv = new MockServer()
    createServer.mockImplementationOnce(() => srv)
    inEp.pollActive = true
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()

    const closed = jest.fn()
    bridge.on('closed', closed)
    await bridge.stop()
    expect(inEp.stopPoll).toHaveBeenCalled()
    expect(iface.release).toHaveBeenCalled()
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
    const { dev } = deviceWithInterface()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const t0 = Date.now()
    await bridge.drain(100)
    expect(Date.now() - t0).toBeLessThan(500)
  })
})

// (skip the full non-accessory boot test — exercises libusb re-enumeration
// internals that aren't worth simulating in a unit test)

describe('UsbAoapBridge — forceReenum', () => {
  test('no-op when nothing has been started', async () => {
    const dev = new MockDevice()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.forceReenum()).resolves.toBeUndefined()
  })

  test('after a successful start, forceReenum tears down endpoints + server', async () => {
    const { dev, iface, inEp } = deviceWithInterface()
    const srv = new MockServer()
    createServer.mockImplementationOnce(() => srv)
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    inEp.stopPoll.mockClear()
    await bridge.forceReenum()
    expect(inEp.stopPoll).toHaveBeenCalled()
    expect(iface.release).toHaveBeenCalled()
    expect(srv.close).toHaveBeenCalled()
  })

  test('forceReenum control-transfer failure is swallowed', async () => {
    const { dev } = deviceWithInterface()
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    dev.controlTransfer.mockImplementation(
      (
        _bm: number,
        _br: number,
        _wv: number,
        _wi: number,
        _data: Buffer | number,
        cb: (err: Error | null, data?: Buffer) => void
      ) => {
        process.nextTick(() => cb(new Error('stalled')))
      }
    )
    await expect(bridge.forceReenum()).resolves.toBeUndefined()
  })
})

describe('UsbAoapBridge — loopback server + pump', () => {
  class MockLoopbackSocket extends EventEmitter {
    setNoDelay = jest.fn()
    write = jest.fn(() => true)
    destroy = jest.fn()
    once(event: string, listener: (...args: unknown[]) => void): this {
      super.once(event, listener)
      return this
    }
  }

  function newBridge() {
    const { dev, iface, inEp, outEp } = deviceWithInterface()
    const srv = new MockServer()
    let connHandler: ((s: MockLoopbackSocket) => void) | null = null
    createServer.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
      connHandler = h as (s: MockLoopbackSocket) => void
      return srv
    })
    return { dev, iface, inEp, outEp, srv, connect: () => connHandler! }
  }

  test('client connect → setNoDelay + startPump → polling kicks off', async () => {
    const { dev, inEp, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    expect(sock.setNoDelay).toHaveBeenCalledWith(true)
    expect(inEp.startPoll).toHaveBeenCalled()
  })

  test('second client tears down the first', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const a = new MockLoopbackSocket()
    connect()(a)
    const b = new MockLoopbackSocket()
    connect()(b)
    expect(a.destroy).toHaveBeenCalled()
  })

  test('USB IN → socket write', async () => {
    const { dev, inEp, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    inEp.emit('data', Buffer.from([1, 2, 3]))
    expect(sock.write).toHaveBeenCalledWith(Buffer.from([1, 2, 3]))
  })

  test('socket → USB OUT.transfer', async () => {
    const { dev, outEp, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    sock.emit('data', Buffer.from([0xaa]))
    await new Promise((r) => setImmediate(r))
    expect(outEp.transfer).toHaveBeenCalled()
  })

  test('USB IN error → emit error + destroy socket', async () => {
    const { dev, inEp, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const onErr = jest.fn()
    bridge.on('error', onErr)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    inEp.emit('error', new Error('libusb timeout'))
    expect(onErr).toHaveBeenCalled()
    expect(sock.destroy).toHaveBeenCalled()
  })

  test('socket close pauses the pump and clears _client', async () => {
    const { dev, inEp, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    inEp.stopPoll.mockClear()
    sock.emit('close')
    expect(inEp.stopPoll).toHaveBeenCalled()
  })

  test('socket error is forwarded', async () => {
    const { dev, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const onErr = jest.fn()
    bridge.on('error', onErr)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    sock.emit('error', new Error('reset'))
    expect(onErr).toHaveBeenCalled()
  })
})

describe('UsbAoapBridge — accessory open retry', () => {
  test('first 4 opens throw, fifth succeeds', async () => {
    const { dev } = deviceWithInterface()
    let attempts = 0
    dev.open.mockImplementation(() => {
      attempts++
      if (attempts < 5) throw new Error('udev not ready')
    })
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    expect(dev.open).toHaveBeenCalledTimes(5)
  })

  test('5 failed opens throw with the last error message', async () => {
    isAccessoryModeMock.mockReturnValue(true)
    const dev = new MockDevice()
    dev.open.mockImplementation(() => {
      throw new Error('udev not ready')
    })
    dev.interface.mockReturnValue(null)
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.start()).rejects.toThrow(/Failed to open AOAP accessory/)
  })

  test('claim retry: first call throws, second succeeds', async () => {
    const { dev, iface } = deviceWithInterface()
    let attempts = 0
    iface.claim.mockImplementation(() => {
      attempts++
      if (attempts < 2) throw new Error('udev claim race')
    })
    createServer.mockImplementationOnce(() => new MockServer())
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    expect(iface.claim).toHaveBeenCalledTimes(2)
  })

  test('5 failed claims throw a descriptive error', async () => {
    const { dev, iface } = deviceWithInterface()
    iface.claim.mockImplementation(() => {
      throw new Error('busy')
    })
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await expect(bridge.start()).rejects.toThrow(/Failed to claim AOAP accessory/)
  })
})

describe('UsbAoapBridge — pump edge cases', () => {
  class MockLoopbackSocket extends EventEmitter {
    setNoDelay = jest.fn()
    write = jest.fn(() => true)
    destroy = jest.fn()
    once(event: string, listener: (...args: unknown[]) => void): this {
      super.once(event, listener)
      return this
    }
  }

  function newBridge() {
    const dev = new MockDevice()
    const iface = new MockInterface()
    const inEp = Object.assign(new MockEndpoint(), { transferType: 0x02, address: 0x81 })
    const outEp = Object.assign(new MockEndpoint(), { transferType: 0x02, address: 0x02 })
    iface.endpoints = [inEp, outEp]
    dev.interface.mockReturnValue(iface)
    const srv = new MockServer()
    let connHandler: ((s: MockLoopbackSocket) => void) | null = null
    createServer.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
      connHandler = h as (s: MockLoopbackSocket) => void
      return srv
    })
    return { dev, iface, inEp, outEp, srv, connect: () => connHandler! }
  }

  test('USB IN with backpressure pauses + resumes on drain', async () => {
    const { dev, inEp, connect } = newBridge()
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    sock.write = jest.fn(() => false) // signal backpressure
    connect()(sock)
    inEp.stopPoll.mockClear()
    inEp.startPoll.mockClear()
    inEp.emit('data', Buffer.from([1]))
    expect(inEp.stopPoll).toHaveBeenCalled()
    sock.emit('drain')
    expect(inEp.startPoll).toHaveBeenCalled()
  })

  test('outChain transfer error → emit error + destroy socket', async () => {
    const { dev, outEp, connect } = newBridge()
    outEp.transfer = jest.fn((_b: Buffer, cb: (err?: Error) => void) => cb(new Error('USB stall')))
    const bridge = new UsbAoapBridge(dev as unknown as Device)
    const onErr = jest.fn()
    bridge.on('error', onErr)
    await bridge.start()
    const sock = new MockLoopbackSocket()
    connect()(sock)
    sock.emit('data', Buffer.from([0xaa]))
    await new Promise((r) => setImmediate(r))
    expect(onErr).toHaveBeenCalled()
    expect(sock.destroy).toHaveBeenCalled()
  })
})
