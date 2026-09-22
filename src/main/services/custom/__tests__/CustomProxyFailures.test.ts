import { EventEmitter } from 'events'

const listen = vi.fn()
const close = vi.fn()
const closeAllConnections = vi.fn()

class FakeServer extends EventEmitter {
  listen = listen
  close = close
  closeAllConnections = closeAllConnections
  address = () => null
}

let server: FakeServer
let onRequest: ((req: unknown, res: unknown) => void) | undefined
const request = vi.fn()

vi.mock('http', () => ({
  createServer: vi.fn((handler: (req: unknown, res: unknown) => void) => {
    onRequest = handler
    return server
  }),
  request
}))

const { CustomProxy } = await import('@main/services/custom/CustomProxy')

describe('CustomProxy failures', () => {
  beforeEach(() => {
    server = new FakeServer()
    listen.mockReset()
    close.mockReset()
    closeAllConnections.mockReset()
    request.mockReset()
    onRequest = undefined
  })

  test('gives up when the loopback server cannot listen', async () => {
    listen.mockImplementation(() => server.emit('error', new Error('EADDRINUSE')))

    const proxy = new CustomProxy()

    expect(await proxy.start('http://10.0.0.9/')).toBeNull()
    expect(proxy.url()).toBeNull()
    expect(close).toHaveBeenCalled()
  })

  test('gives up when the server names no address', async () => {
    listen.mockImplementation((_port: number, _host: string, cb: () => void) => cb())

    const proxy = new CustomProxy()

    expect(await proxy.start('http://10.0.0.9/')).toBeNull()
    expect(close).toHaveBeenCalled()
  })

  test('logs an error the running server reports', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    listen.mockImplementation(() => undefined)

    const proxy = new CustomProxy()
    void proxy.start('http://10.0.0.9/')
    server.emit('error', new Error('boom'))

    expect(warn).toHaveBeenCalledWith(expect.stringContaining('boom'))
    warn.mockRestore()
  })

  test('answers 502 when the target names no status', async () => {
    listen.mockImplementation((_port: number, _host: string, cb: () => void) => cb())
    server.address = () => ({ port: 1234 }) as never
    const remote = Object.assign(new EventEmitter(), { headers: {}, pipe: vi.fn() })
    request.mockImplementation((_opts: unknown, cb: (r: unknown) => void) => {
      cb(remote)
      return Object.assign(new EventEmitter(), { end: vi.fn() })
    })

    const proxy = new CustomProxy()
    await proxy.start('http://10.0.0.9/')

    const res = { writeHead: vi.fn(), end: vi.fn() }
    onRequest?.(Object.assign(new EventEmitter(), { headers: {}, url: '/', pipe: vi.fn() }), res)

    expect(res.writeHead).toHaveBeenCalledWith(502, expect.any(Object))
  })
})
