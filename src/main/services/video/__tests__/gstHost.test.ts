import { EventEmitter } from 'node:events'

const { spawnMock, children } = vi.hoisted(() => {
  const events = require('node:events')
  const children: Array<
    InstanceType<typeof events.EventEmitter> & { kill: ReturnType<typeof vi.fn> }
  > = []
  const spawnMock = vi.fn(() => {
    const c = new events.EventEmitter()
    c.kill = vi.fn()
    children.push(c)
    return c
  })
  return { spawnMock, children }
})
vi.mock('node:child_process', () => ({ spawn: spawnMock }))

const { createServerMock, servers, connectionHandlers } = vi.hoisted(() => {
  const events = require('node:events')
  const servers: Array<InstanceType<typeof events.EventEmitter>> = []
  const connectionHandlers: Array<(s: unknown) => void> = []
  const createServerMock = vi.fn((handler: (s: unknown) => void) => {
    connectionHandlers.push(handler)
    const server = new events.EventEmitter()
    server.listen = vi.fn((_path: string, cb?: () => void) => {
      cb?.()
      return server
    })
    server.close = vi.fn()
    servers.push(server)
    return server
  })
  return { createServerMock, servers, connectionHandlers }
})
vi.mock('node:net', () => ({
  default: { createServer: createServerMock },
  createServer: createServerMock
}))

const { fs } = vi.hoisted(() => ({
  fs: {
    chmodSync: vi.fn(),
    unlinkSync: vi.fn(),
    existsSync: vi.fn(() => false),
    readFileSync: vi.fn(() => '')
  }
}))
vi.mock('node:fs', () => fs)

vi.mock('node:os', () => ({ default: { tmpdir: () => '/tmp' }, tmpdir: () => '/tmp' }))

const { appOn } = vi.hoisted(() => ({ appOn: vi.fn() }))
vi.mock('electron', () => ({ app: { on: appOn } }))

const { resolveRootMock, gstEnvMock } = vi.hoisted(() => ({
  resolveRootMock: vi.fn((): string | null => '/opt/gst'),
  gstEnvMock: vi.fn((root: string) => ({
    GST_PLUGIN_SYSTEM_PATH: '',
    GST_PLUGIN_PATH: `${root}/lib/gstreamer-1.0`,
    LD_LIBRARY_PATH: `${root}/lib`
  }))
}))
vi.mock('../../audio/gstreamer', () => ({
  resolveGStreamerRoot: resolveRootMock,
  gstEnv: gstEnvMock
}))

type GstHostModule = typeof import('../gstHost')

async function freshHost(): Promise<GstHostModule['gstHost']> {
  vi.resetModules()
  return (await import('../gstHost')).gstHost
}

function makeSocket() {
  const s = new EventEmitter() as EventEmitter & {
    writable: boolean
    write: ReturnType<typeof vi.fn>
  }
  s.writable = true
  s.write = vi.fn()
  return s
}

beforeEach(() => {
  vi.clearAllMocks()
  children.length = 0
  servers.length = 0
  connectionHandlers.length = 0
  fs.existsSync.mockReturnValue(false)
  fs.readFileSync.mockReturnValue('')
})

describe('gstHost framing + transport', () => {
  test('starts the host and queues a create frame until the socket connects', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(7, 'h264')

    expect(createServerMock).toHaveBeenCalledTimes(1)
    expect(spawnMock).toHaveBeenCalledTimes(1)

    const sock = makeSocket()
    connectionHandlers[0](sock)

    expect(sock.write).toHaveBeenCalledTimes(1)
    const f = sock.write.mock.calls[0][0] as Buffer
    expect(f.readUInt32LE(0)).toBe(5 + 1 + 'h264'.length) // [len]: op+id + [1B codecLen] + codec
    expect(f.readUInt8(4)).toBe(1) // op = create
    expect(f.readUInt32LE(5)).toBe(7) // id
    expect(f.readUInt8(9)).toBe('h264'.length) // codec length prefix
    expect(f.subarray(10).toString('utf8')).toBe('h264')
  })

  test('writes directly once the socket is live', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')
    const sock = makeSocket()
    connectionHandlers[0](sock)
    sock.write.mockClear()

    gstHost.pushBuffer(1, Buffer.from([0xaa, 0xbb]))

    expect(sock.write).toHaveBeenCalledTimes(1)
    const f = sock.write.mock.calls[0][0] as Buffer
    expect(f.readUInt8(4)).toBe(2) // op = data
    expect(f.readUInt32LE(5)).toBe(1)
    expect(f.subarray(9)).toEqual(Buffer.from([0xaa, 0xbb]))
  })

  test('stop sends an empty-payload frame', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')
    const sock = makeSocket()
    connectionHandlers[0](sock)
    sock.write.mockClear()

    gstHost.stop(3)

    const f = sock.write.mock.calls[0][0] as Buffer
    expect(f.readUInt32LE(0)).toBe(5) // header only
    expect(f.readUInt8(4)).toBe(3) // op = stop
    expect(f.readUInt32LE(5)).toBe(3)
    expect(f).toHaveLength(9)
  })

  test('flushes all queued frames in order on connect', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')
    gstHost.pushBuffer(1, Buffer.from([0x01]))
    gstHost.stop(1)

    const sock = makeSocket()
    connectionHandlers[0](sock)

    expect(sock.write).toHaveBeenCalledTimes(3)
    const ops = sock.write.mock.calls.map((c) => (c[0] as Buffer).readUInt8(4))
    expect(ops).toEqual([1, 2, 3])
  })

  test('start is idempotent — the host is spawned once', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')
    gstHost.createPlayer(2, 'h264')

    expect(spawnMock).toHaveBeenCalledTimes(1)
    expect(createServerMock).toHaveBeenCalledTimes(1)
  })

  test('spawns the host on the bundled GStreamer', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')

    const env = spawnMock.mock.calls[0]![2].env
    expect(env.LD_LIBRARY_PATH).toBe('/opt/gst/lib')
    expect(env.GST_PLUGIN_PATH).toBe('/opt/gst/lib/gstreamer-1.0')
    expect(env.GST_PLUGIN_SYSTEM_PATH).toBe('')
    expect(env.GST_GL_WINDOW).toBe('surfaceless')
    expect(env.GST_GL_PLATFORM).toBe('egl')
  })

  test('spawns with the plain environment when no bundle is present', async () => {
    resolveRootMock.mockReturnValueOnce(null)
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')

    const env = spawnMock.mock.calls[0]![2].env
    expect(env.LD_LIBRARY_PATH).toBeUndefined()
    expect(env.GST_GL_WINDOW).toBe('surfaceless')
  })

  test('a socket close clears the active socket so later frames re-queue', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')
    const sock = makeSocket()
    connectionHandlers[0](sock)
    sock.write.mockClear()

    sock.emit('close')
    gstHost.pushBuffer(1, Buffer.from([0x01]))

    // socket gone → frame is queued, not written
    expect(sock.write).not.toHaveBeenCalled()
  })
})

describe('gstHost child lifecycle', () => {
  test('child exit on a signal prints the crash backtrace and closes the server', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    fs.existsSync.mockReturnValue(true)
    fs.readFileSync.mockReturnValue('==== backtrace ====')

    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')

    children[0].emit('exit', null, 'SIGSEGV')

    expect(fs.readFileSync).toHaveBeenCalled()
    expect(servers[0].close).toHaveBeenCalled()
    errSpy.mockRestore()
  })

  test('a clean exit (no signal) does not read a crash log', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')

    children[0].emit('exit', 0, null)

    expect(fs.readFileSync).not.toHaveBeenCalled()
    errSpy.mockRestore()
  })

  test('respawns the host after the child exited', async () => {
    const errSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')
    expect(spawnMock).toHaveBeenCalledTimes(1)

    children[0].emit('exit', 0, null)
    gstHost.createPlayer(2, 'h264')

    expect(spawnMock).toHaveBeenCalledTimes(2)
    errSpy.mockRestore()
  })

  test('the before-quit hook kills the child', async () => {
    const gstHost = await freshHost()
    gstHost.createPlayer(1, 'h264')

    const hook = appOn.mock.calls.find((c) => c[0] === 'before-quit')?.[1] as () => void
    expect(hook).toBeTypeOf('function')
    hook()
    expect(children[0].kill).toHaveBeenCalled()
  })
})
