import { EventEmitter } from 'node:events'

class MockNetSocket extends EventEmitter {
  remoteAddress = '127.0.0.1'
  remotePort = 12345
  setNoDelay = jest.fn()
  setTimeout = jest.fn()
}

class MockServer extends EventEmitter {
  listen = jest.fn((_port: number, _addr: string, cb: () => void) => cb())
  close = jest.fn()
}

const createServerMock = jest.fn()
jest.mock('net', () => ({
  __esModule: true,
  createServer: (...args: unknown[]) => createServerMock(...args)
}))

class MockSession extends EventEmitter {
  start = jest.fn(async () => undefined)
  close = jest.fn()
}

const sessionCtor = jest.fn()
jest.mock('../../session/Session', () => ({
  Session: jest.fn().mockImplementation((sock: unknown, cfg: unknown) => {
    sessionCtor(sock, cfg)
    return new MockSession()
  })
}))

import { TcpServer } from '../TcpServer'

beforeEach(() => {
  createServerMock.mockReset()
  sessionCtor.mockReset()
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('TcpServer', () => {
  test('listen() opens a net.createServer on the supplied port', () => {
    let handler: ((s: unknown) => void) | null = null
    const srv = new MockServer()
    createServerMock.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
      handler = h
      return srv
    })

    const tcp = new TcpServer({} as never)
    tcp.listen(5555)
    expect(srv.listen).toHaveBeenCalledWith(5555, '0.0.0.0', expect.any(Function))
    void handler
  })

  test('an inbound connection wires a Session and emits "session"', () => {
    let connHandler: ((s: MockNetSocket) => void) | null = null
    const srv = new MockServer()
    createServerMock.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
      connHandler = h as (s: MockNetSocket) => void
      return srv
    })

    const tcp = new TcpServer({} as never)
    const sessionCb = jest.fn()
    tcp.on('session', sessionCb)
    tcp.listen()

    const sock = new MockNetSocket()
    connHandler!(sock)
    expect(sock.setNoDelay).toHaveBeenCalledWith(true)
    expect(sessionCtor).toHaveBeenCalled()
    expect(sessionCb).toHaveBeenCalled()
  })

  test('close() closes the server', () => {
    const srv = new MockServer()
    createServerMock.mockImplementationOnce(() => srv)
    const tcp = new TcpServer({} as never)
    tcp.listen()
    tcp.close()
    expect(srv.close).toHaveBeenCalled()
  })

  test('session error/disconnected events are logged with the remote address', () => {
    const errorLog = jest.spyOn(console, 'error').mockImplementation(() => {})
    const log = jest.spyOn(console, 'log').mockImplementation(() => {})

    let connHandler: ((s: MockNetSocket) => void) | null = null
    const srv = new MockServer()
    createServerMock.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
      connHandler = h as (s: MockNetSocket) => void
      return srv
    })

    const tcp = new TcpServer({} as never)
    tcp.listen()
    const sock = new MockNetSocket()
    connHandler!(sock)
    const session = (jest.requireMock('../../session/Session') as { Session: jest.Mock }).Session
      .mock.results[0].value as MockSession
    session.emit('error', new Error('reset'))
    session.emit('disconnected', 'phone closed')
    session.emit('disconnected') // no reason → falls back to ''

    expect(errorLog).toHaveBeenCalled()
    expect(log).toHaveBeenCalled()
    errorLog.mockRestore()
    log.mockRestore()
  })

  test('session.start rejection is logged but never throws out of the listener', async () => {
    const errorLog = jest.spyOn(console, 'error').mockImplementation(() => {})
    const { Session } = jest.requireMock('../../session/Session') as { Session: jest.Mock }
    Session.mockImplementationOnce(() => {
      const s = new MockSession()
      s.start = jest.fn(async () => {
        throw new Error('TLS broken')
      })
      return s
    })

    let connHandler: ((s: MockNetSocket) => void) | null = null
    const srv = new MockServer()
    createServerMock.mockImplementationOnce((_opts: unknown, h: (s: unknown) => void) => {
      connHandler = h as (s: MockNetSocket) => void
      return srv
    })

    const tcp = new TcpServer({} as never)
    tcp.listen()
    expect(() => connHandler!(new MockNetSocket())).not.toThrow()
    await new Promise((r) => setImmediate(r))
    expect(errorLog).toHaveBeenCalledWith(
      expect.stringContaining('start error'),
      expect.any(String)
    )
    errorLog.mockRestore()
  })

  test('server "error" event is re-emitted', () => {
    const srv = new MockServer()
    createServerMock.mockImplementationOnce(() => srv)
    const tcp = new TcpServer({} as never)
    const onError = jest.fn()
    tcp.on('error', onError)
    tcp.listen()
    srv.emit('error', new Error('eaddrinuse'))
    expect(onError).toHaveBeenCalled()
  })
})
