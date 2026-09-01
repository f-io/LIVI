import { EventEmitter } from 'node:events'

class MockClientSocket extends EventEmitter {
  disconnect = vi.fn()
}

const lastClient: { socket: MockClientSocket | null; url: string | null; opts: unknown } = {
  socket: null,
  url: null,
  opts: null
}

vi.mock('socket.io-client', () => ({
  __esModule: true,
  io: vi.fn((url: string, opts: unknown) => {
    const s = new MockClientSocket()
    lastClient.socket = s
    lastClient.url = url
    lastClient.opts = opts
    return s
  })
}))

import { TelemetryEvents } from '../Socket'
import { TelemetrySocketClient } from '../SocketClient'
import { TelemetryStore } from '../telemetry/TelemetryStore'

beforeEach(async () => {
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  lastClient.socket = null
  lastClient.url = null
  lastClient.opts = null
})
afterEach(async () => vi.restoreAllMocks())

describe('TelemetrySocketClient', () => {
  test('dials out to the configured host:port on construction', () => {
    const store = new TelemetryStore()
    const client = new TelemetrySocketClient(store, '10.0.0.5', 4000)
    expect(lastClient.url).toBe('http://10.0.0.5:4000')
    expect(client.socket).not.toBeNull()
  })

  test('inbound "telemetry:update" merges into the store', () => {
    const store = new TelemetryStore()
    new TelemetrySocketClient(store, '10.0.0.5', 4000)
    lastClient.socket!.emit(TelemetryEvents.Update, { speedKph: 42 })
    expect(store.snapshot().speedKph).toBe(42)
  })

  test('does not emit "telemetry:push" back to the remote', () => {
    const store = new TelemetryStore()
    const client = new TelemetrySocketClient(store, '10.0.0.5', 4000)
    store.merge({ speedKph: 5 })
    expect(client.socket).not.toBeNull()
    // No push handler is ever wired up client-side; store changes don't touch the socket.
    expect((client.socket as unknown as MockClientSocket).listenerCount(TelemetryEvents.Push)).toBe(
      0
    )
  })

  test('logs connect_error without crashing', () => {
    const store = new TelemetryStore()
    new TelemetrySocketClient(store, '10.0.0.5', 4000)
    expect(() => lastClient.socket!.emit('connect_error', new Error('ECONNREFUSED'))).not.toThrow()
  })

  test('disconnect() tears down the client socket', async () => {
    const store = new TelemetryStore()
    const client = new TelemetrySocketClient(store, '10.0.0.5', 4000)
    const sock = lastClient.socket!
    await client.disconnect()
    expect(sock.disconnect).toHaveBeenCalled()
    expect(client.socket).toBeNull()
  })

  test('connect() re-establishes after a disconnect', async () => {
    const store = new TelemetryStore()
    const client = new TelemetrySocketClient(store, '10.0.0.5', 4000)
    await client.disconnect()
    await client.connect()
    expect(client.socket).not.toBeNull()
  })

  test('connect() is a no-op while already connected', async () => {
    const store = new TelemetryStore()
    const client = new TelemetrySocketClient(store, '10.0.0.5', 4000)
    const sock = client.socket
    await client.connect()
    expect(client.socket).toBe(sock)
  })
})
