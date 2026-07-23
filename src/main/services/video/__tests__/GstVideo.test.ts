const { netConnect, sockets } = vi.hoisted(() => {
  const events = require('node:events')
  const sockets: Array<
    InstanceType<typeof events.EventEmitter> & { write: ReturnType<typeof vi.fn> }
  > = []
  const netConnect = vi.fn(() => {
    const s = new events.EventEmitter()
    s.destroyed = false
    s.writable = true
    s.write = vi.fn()
    sockets.push(s)
    return s
  })
  return { netConnect, sockets }
})
vi.mock('node:net', () => ({ default: { connect: netConnect }, connect: netConnect }))

const { gstHost } = vi.hoisted(() => ({
  gstHost: { createPlayer: vi.fn(), pushBuffer: vi.fn(), stop: vi.fn(), setGamma: vi.fn() }
}))
vi.mock('../gstHost', () => ({ gstHost }))

vi.mock('../../audio/gstreamer', () => ({
  resolveGStreamerRoot: vi.fn(() => '/gst'),
  resolveBinary: vi.fn(() => '/gst/bin/gst-launch-1.0'),
  gstEnv: vi.fn(() => ({}))
}))

const { addon } = vi.hoisted(() => ({
  addon: {
    version: vi.fn(() => '1.0-test'),
    probeCodecs: vi.fn(() => ({
      h264: { hw: true, sw: true },
      h265: { hw: false, sw: true },
      vp9: { hw: false, sw: true },
      av1: { hw: false, sw: true }
    })),
    createPlayer: vi.fn(() => ({ handle: 1 })),
    start: vi.fn(),
    pushBuffer: vi.fn(() => true),
    setVisible: vi.fn(),
    setContentRegion: vi.fn(),
    setBackdrop: vi.fn(),
    setGamma: vi.fn(),
    stop: vi.fn()
  }
}))
vi.mock('gst-video', () => addon)

const { win } = vi.hoisted(() => ({
  win: {
    isDestroyed: vi.fn(() => false),
    getNativeWindowHandle: vi.fn(() => Buffer.from([1, 2, 3, 4]))
  }
}))
vi.mock('electron', () => ({
  app: { isPackaged: false },
  BrowserWindow: Object.assign(vi.fn(), { fromWebContents: vi.fn(() => win) })
}))

type GstVideoModule = typeof import('../GstVideo')

const originalPlatform = process.platform

async function loadModule(platform = 'linux', ctrl?: string): Promise<GstVideoModule> {
  vi.resetModules()
  Object.defineProperty(process, 'platform', { value: platform, configurable: true })
  if (ctrl === undefined) delete process.env.LIVI_COMPOSITOR_CTRL
  else process.env.LIVI_COMPOSITOR_CTRL = ctrl
  return import('../GstVideo')
}

beforeEach(() => {
  vi.clearAllMocks()
  sockets.length = 0
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  delete process.env.LIVI_COMPOSITOR_CTRL
})

describe('compositor control (linux + control path)', () => {
  test('connects to the control socket and writes the backdrop line on connect', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#ff8000')

    expect(netConnect).toHaveBeenCalledWith('/sock')
    const sock = sockets[0]
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('backdrop 255 128 0\n')
  })

  test('writes a sized and an unsized screen line', async () => {
    const m = await loadModule('linux', '/sock')

    m.setCompositorScreen('dash', true, 800, 480)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('screen dash 1 800 480\n')

    m.setCompositorScreen('dash', false)
    expect(sockets[0].write).toHaveBeenCalledWith('screen dash 0\n')
  })

  test('restart returns true and queues the restart line', async () => {
    const m = await loadModule('linux', '/sock')

    expect(m.compositorRestart()).toBe(true)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('restart\n')
  })

  test('resends sticky state on a live socket', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    const sock = sockets[0]
    sock.emit('connect')
    sock.write.mockClear()

    m.setCompositorScreen('dash', true)
    expect(sock.write).toHaveBeenCalledWith('screen dash 1\n')
    expect(sock.write).toHaveBeenCalledWith('backdrop 0 0 0\n')
  })

  test('survives socket error and close events', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    const sock = sockets[0]

    expect(() => {
      sock.emit('error')
      sock.emit('close')
    }).not.toThrow()
  })

  test('malformed backdrop hex falls back to black', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('not-a-color')
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('backdrop 0 0 0\n')
  })
})

describe('compositor control disabled', () => {
  test('no-ops without a control path', async () => {
    const m = await loadModule('linux', undefined)
    m.setCompositorBackdrop('#fff')
    m.setCompositorScreen('dash', true)
    expect(m.compositorRestart()).toBe(false)
    expect(netConnect).not.toHaveBeenCalled()
  })

  test('no-ops on a non-linux platform', async () => {
    const m = await loadModule('darwin', '/sock')
    m.setCompositorBackdrop('#fff')
    expect(m.compositorRestart()).toBe(false)
    expect(netConnect).not.toHaveBeenCalled()
  })
})

describe('GstVideo — linux host-process path', () => {
  test('creates a host player, claims a plane and forwards buffers', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'main', 'main')

    v.push('h264', Buffer.from([1, 2, 3]))

    expect(gstHost.createPlayer).toHaveBeenCalledWith(expect.any(Number), 'h264', undefined)
    expect(gstHost.pushBuffer).toHaveBeenCalledTimes(1)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('claim main\n')
  })

  test('ensure is idempotent for an unchanged codec', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    v.push('h264', Buffer.from([2]))
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(1)
    expect(gstHost.pushBuffer).toHaveBeenCalledTimes(2)
  })

  test('switching the codec disposes and recreates the player', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    v.push('h265', Buffer.from([2]))
    expect(gstHost.stop).toHaveBeenCalledTimes(1)
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(2)
  })

  test('setVisible toggles the compositor plane', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'dash')
    v.setVisible(false)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('videoshow dash 0\n')
  })

  test('setContentRegion sends a videocfg line', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'main', 'aux')
    v.setContentRegion(0, 0, 800, 480, 1920, 1080)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('videocfg main aux 0 0 800 480 1920 1080\n')
  })

  test('dispose stops the host player', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    v.dispose()
    expect(gstHost.stop).toHaveBeenCalledTimes(1)
  })
})

describe('probe + backdrop helpers', () => {
  test('backdropHex resolves dark/light with theme fallbacks', async () => {
    const m = await loadModule()
    expect(m.backdropHex(true, '#111111', '#eeeeee')).toBe('#111111')
    expect(m.backdropHex(false, '#111111', '#eeeeee')).toBe('#eeeeee')
    expect(m.backdropHex(true)).toBe('#000000')
    expect(m.backdropHex(false)).toBe('#d4d4d4')
  })

  test('setMacBackdrop is a no-op off darwin', async () => {
    const m = await loadModule('linux')
    const fakeWin = { isDestroyed: () => false, getNativeWindowHandle: () => Buffer.from([1]) }
    expect(() => m.setMacBackdrop(fakeWin as never, '#ffffff')).not.toThrow()
  })
})
