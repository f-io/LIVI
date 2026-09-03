import Module from 'node:module'

const { netConnect, sockets } = vi.hoisted(() => {
  const events = require('node:events')
  const sockets: Array<
    InstanceType<typeof events.EventEmitter> & {
      destroyed: boolean
      writable: boolean
      write: ReturnType<typeof vi.fn>
    }
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

const { gstHost, probeViaHostMock } = vi.hoisted(() => ({
  gstHost: {
    createPlayer: vi.fn(),
    pushBuffer: vi.fn(),
    stop: vi.fn(),
    setGamma: vi.fn(),
    openFeed: vi.fn(() => Promise.resolve('/host/feed'))
  },
  probeViaHostMock: vi.fn((): Record<string, { hw: boolean; sw: boolean }> | null => null)
}))
vi.mock('../gstHost', () => ({ gstHost, probeCodecsViaHost: probeViaHostMock }))

const { execFileSyncMock } = vi.hoisted(() => ({
  execFileSyncMock: vi.fn(() => 'GStreamer 1.24.5\n')
}))
vi.mock('node:child_process', () => ({ execFileSync: execFileSyncMock }))

const { resolveRootMock, resolveBinaryMock, gstEnvMock } = vi.hoisted(() => ({
  resolveRootMock: vi.fn((): string | null => '/gst'),
  resolveBinaryMock: vi.fn((): string | null => '/gst/bin/gst-launch-1.0'),
  gstEnvMock: vi.fn(() => ({}))
}))
vi.mock('../../audio/gstreamer', () => ({
  resolveGStreamerRoot: resolveRootMock,
  resolveBinary: resolveBinaryMock,
  gstEnv: gstEnvMock
}))

const { addon, loadState } = vi.hoisted(() => ({
  addon: {
    version: vi.fn(() => '1.0-test'),
    probeCodecs: vi.fn(() => ({
      h264: { hw: true, sw: true },
      h265: { hw: false, sw: true },
      vp9: { hw: false, sw: true },
      av1: { hw: false, sw: true }
    })),
    createPlayer: vi.fn((): unknown => ({ handle: 1 })),
    start: vi.fn(),
    pushBuffer: vi.fn(() => true),
    setVisible: vi.fn(),
    setContentRegion: vi.fn(),
    setBackdrop: vi.fn() as unknown,
    setGamma: vi.fn(),
    stop: vi.fn(),
    openFeed: vi.fn(() => true)
  },
  loadState: { fail: false }
}))

const { win, electronApp } = vi.hoisted(() => ({
  win: {
    isDestroyed: vi.fn(() => false),
    getNativeWindowHandle: vi.fn(() => Buffer.from([1, 2, 3, 4]))
  },
  electronApp: { isPackaged: false }
}))
vi.mock('electron', () => ({
  app: electronApp,
  BrowserWindow: Object.assign(vi.fn(), { fromWebContents: vi.fn(() => win) })
}))

import { BrowserWindow } from 'electron'

const fromWebContents = (BrowserWindow as unknown as { fromWebContents: ReturnType<typeof vi.fn> })
  .fromWebContents

type GstVideoModule = typeof import('../GstVideo')
type Loader = { _load: (...args: unknown[]) => unknown }

const originalPlatform = process.platform
const M = Module as unknown as Loader
const originalLoad = M._load

beforeAll(() => {
  M._load = function (request: unknown, ...rest: unknown[]) {
    if (request === 'livi-gst-video') {
      if (loadState.fail) throw new Error('addon unavailable')
      return addon
    }
    return originalLoad.call(this, request, ...rest)
  }
})

afterAll(() => {
  M._load = originalLoad
})

async function loadModule(platform = 'linux', ctrl?: string): Promise<GstVideoModule> {
  vi.resetModules()
  Object.defineProperty(process, 'platform', { value: platform, configurable: true })
  if (ctrl === undefined) delete process.env.LIVI_COMPOSITOR_CTRL
  else process.env.LIVI_COMPOSITOR_CTRL = ctrl
  return import('../GstVideo')
}

let logSpy: ReturnType<typeof vi.spyOn>
let warnSpy: ReturnType<typeof vi.spyOn>
let errorSpy: ReturnType<typeof vi.spyOn>

beforeEach(() => {
  vi.clearAllMocks()
  sockets.length = 0
  loadState.fail = false
  electronApp.isPackaged = false
  resolveRootMock.mockImplementation(() => '/gst')
  resolveBinaryMock.mockImplementation(() => '/gst/bin/gst-launch-1.0')
  execFileSyncMock.mockImplementation(() => 'GStreamer 1.24.5\n')
  addon.createPlayer.mockImplementation(() => ({ handle: 1 }))
  logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
  warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
  errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(() => {
  logSpy.mockRestore()
  warnSpy.mockRestore()
  errorSpy.mockRestore()
  vi.useRealTimers()
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  delete process.env.LIVI_COMPOSITOR_CTRL
  delete process.env.GST_PLUGIN_SYSTEM_PATH
  delete process.env.GST_PLUGIN_PATH
  delete process.env.GST_PLUGIN_SCANNER
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

  test('degenerate screen sizes are dropped from the line', async () => {
    const m = await loadModule('linux', '/sock')

    m.setCompositorScreen('a', true, 800, 0)
    m.setCompositorScreen('b', true, -1, 480)
    m.setCompositorScreen('c', true, 800, -1)
    sockets[0].emit('connect')

    expect(sockets[0].write).toHaveBeenCalledWith('screen a 1\n')
    expect(sockets[0].write).toHaveBeenCalledWith('screen b 1\n')
    expect(sockets[0].write).toHaveBeenCalledWith('screen c 1\n')
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

  test('a second flush while connecting reuses the pending socket', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    m.setCompositorScreen('dash', true)

    expect(netConnect).toHaveBeenCalledTimes(1)
  })

  test('reconnects after a socket error', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    sockets[0].emit('error')

    m.setCompositorScreen('dash', true)
    expect(netConnect).toHaveBeenCalledTimes(2)
    sockets[1].emit('connect')
    expect(sockets[1].write).toHaveBeenCalledWith('backdrop 0 0 0\n')
    expect(sockets[1].write).toHaveBeenCalledWith('screen dash 1\n')
  })

  test('a destroyed or unwritable socket triggers a reconnect', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    sockets[0].emit('connect')

    sockets[0].destroyed = true
    m.setCompositorScreen('dash', true)
    expect(netConnect).toHaveBeenCalledTimes(2)

    sockets[1].emit('connect')
    sockets[1].destroyed = false
    sockets[1].writable = false
    m.setCompositorScreen('dash', false)
    expect(netConnect).toHaveBeenCalledTimes(3)
  })

  test('closing a stale socket keeps the live connection', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    sockets[0].emit('error')
    m.setCompositorScreen('dash', true)
    sockets[1].emit('connect')

    sockets[0].emit('close')
    sockets[1].write.mockClear()
    m.compositorRestart()
    expect(sockets[1].write).toHaveBeenCalledWith('restart\n')
    expect(netConnect).toHaveBeenCalledTimes(2)

    sockets[1].emit('close')
    m.setCompositorScreen('dash', false)
    expect(netConnect).toHaveBeenCalledTimes(3)
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

  test('panel lines feed panelPhysicalMm', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    const sock = sockets[0]
    sock.emit('connect')

    sock.emit('data', Buffer.from('panel main 100 50 1000 500\nnoise line\n'))
    sock.emit('data', 'pan')
    sock.emit('data', 'el aux 200 100 100 50\n')

    expect(m.panelPhysicalMm('main', 1000, 500)).toEqual({ widthMm: 100, heightMm: 50 })
    expect(m.panelPhysicalMm('main', 16000, 8000)).toEqual({ widthMm: 800, heightMm: 400 })
    expect(m.panelPhysicalMm('aux', 100, 50)).toEqual({ widthMm: 200, heightMm: 100 })
    expect(m.panelPhysicalMm('aux', 1600, 800)).toEqual({ widthMm: 1600, heightMm: 800 })
    expect(logSpy).toHaveBeenCalledWith("[compositor] panel 'main': 100x50 mm over 1000x500 px")
  })

  test('panelPhysicalMm rejects unknown or degenerate panels', async () => {
    const m = await loadModule('linux', '/sock')
    m.setCompositorBackdrop('#000000')
    const sock = sockets[0]
    sock.emit('connect')
    sock.emit('data', 'panel zx 0 50 1000 500\npanel zy 100 0 1000 500\npanel ok 100 50 1000 500\n')

    expect(m.panelPhysicalMm('missing', 100, 100)).toBeNull()
    expect(m.panelPhysicalMm('zx', 100, 100)).toBeNull()
    expect(m.panelPhysicalMm('zy', 100, 100)).toBeNull()
    expect(m.panelPhysicalMm('ok', 0, 100)).toBeNull()
    expect(m.panelPhysicalMm('ok', 1000, 500)).toEqual({ widthMm: 100, heightMm: 50 })
    expect(m.panelPhysicalMm('ok', 16000, 8000)).toEqual({ widthMm: 800, heightMm: 400 })
  })
})

describe('compositor control disabled', () => {
  test('no-ops without a control path', async () => {
    const m = await loadModule('linux', undefined)
    m.setCompositorBackdrop('#fff')
    m.setCompositorScreen('dash', true)
    m.setStreamGamma(1, 1, 1, 1, 1)
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

  test('notifies the player-created hook once the claim completes', async () => {
    const m = await loadModule('linux', '/sock')
    const created = vi.fn()
    m.setOnPlayerCreated(created)
    const v = new m.GstVideo({} as never, 'main', 'main')
    v.push('h264', Buffer.from([1]))
    expect(created).toHaveBeenCalledTimes(1)
    m.setOnPlayerCreated(null)
  })

  test('an explicit player id is forwarded to the host', async () => {
    const m = await loadModule('linux', undefined)
    const v = new m.GstVideo({} as never, 'main', 'main', 42)
    v.push('h264', Buffer.from([1]))
    expect(gstHost.createPlayer).toHaveBeenCalledWith(42, 'h264', undefined)
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

  test('claims immediately without a compositor control path', async () => {
    const m = await loadModule('linux', undefined)
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(1)
    v.setVisible(true)
    v.dispose()
    expect(gstHost.stop).toHaveBeenCalledTimes(1)
  })

  test('prepare passes the codec_data record to the host player', async () => {
    const m = await loadModule('linux', undefined)
    const v = new m.GstVideo({} as never)
    const cd = Buffer.from([1, 2, 3])
    v.prepare('h265', cd)
    expect(gstHost.createPlayer).toHaveBeenCalledWith(expect.any(Number), 'h265', cd)
  })

  test('prepare without codec_data keeps the record empty', async () => {
    const m = await loadModule('linux', undefined)
    const v = new m.GstVideo({} as never)
    v.prepare('h264')
    expect(gstHost.createPlayer).toHaveBeenCalledWith(expect.any(Number), 'h264', undefined)
  })

  test('buffers pushed while a claim is queued are flushed once it is bound', async () => {
    const m = await loadModule('linux', '/sock')
    const v1 = new m.GstVideo({} as never, 'main')
    const v2 = new m.GstVideo({} as never, 'dash')

    v1.push('h264', Buffer.from([1]))
    v2.push('h264', Buffer.from([2]))
    v2.push('h264', Buffer.from([3]))
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(1)
    expect(gstHost.pushBuffer).toHaveBeenCalledTimes(1)

    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('bound main\n'))
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(2)
    expect(gstHost.pushBuffer).toHaveBeenCalledTimes(3)
    expect(sockets[0].write).toHaveBeenCalledWith('claim dash\n')
  })

  test('the pending buffer queue is capped at 240 frames', async () => {
    const m = await loadModule('linux', '/sock')
    const v1 = new m.GstVideo({} as never, 'main')
    const v2 = new m.GstVideo({} as never, 'dash')
    v1.push('h264', Buffer.from([1]))
    for (let i = 0; i < 241; i++) v2.push('h264', Buffer.from([i]))

    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('bound main\n'))
    expect(gstHost.pushBuffer).toHaveBeenCalledTimes(1 + 240)
  })

  test('a queued claim is dropped when the player is disposed', async () => {
    const m = await loadModule('linux', '/sock')
    const v1 = new m.GstVideo({} as never, 'main')
    const v2 = new m.GstVideo({} as never, 'dash')
    v1.push('h264', Buffer.from([1]))
    v2.push('h264', Buffer.from([2]))

    v2.dispose()
    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('bound main\n'))
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(1)
  })

  test('disposing the active claim unclaims the plane', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'main')
    v.push('h264', Buffer.from([1]))
    v.dispose()

    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('unclaim main\n')
    expect(gstHost.stop).toHaveBeenCalledTimes(1)
  })

  test('an unanswered claim times out and unclaims the plane', async () => {
    vi.useFakeTimers()
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'main')
    v.push('h264', Buffer.from([1]))
    sockets[0].emit('connect')
    sockets[0].write.mockClear()

    vi.advanceTimersByTime(3000)
    expect(sockets[0].write).toHaveBeenCalledWith('unclaim main\n')
  })

  test('a claim timeout after the control got disabled stays silent', async () => {
    vi.useFakeTimers()
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'main')
    v.push('h264', Buffer.from([1]))
    sockets[0].emit('connect')
    sockets[0].write.mockClear()

    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    vi.advanceTimersByTime(3000)
    expect(sockets[0].write).not.toHaveBeenCalledWith('unclaim main\n')
  })

  test('setCodecData recreates a running pipeline when the record changes', async () => {
    const m = await loadModule('linux', undefined)
    const v = new m.GstVideo({} as never)
    const cd1 = Buffer.from([1])
    v.prepare('h264', cd1)
    expect(gstHost.createPlayer).toHaveBeenCalledWith(expect.any(Number), 'h264', cd1)

    v.setCodecData(Buffer.from([1]))
    expect(gstHost.stop).not.toHaveBeenCalled()

    const cd2 = Buffer.from([2])
    v.setCodecData(cd2)
    expect(gstHost.stop).toHaveBeenCalledTimes(1)

    v.push('h264', Buffer.from([9]))
    expect(gstHost.createPlayer).toHaveBeenLastCalledWith(expect.any(Number), 'h264', cd2)
  })

  test('setCodecData before the first frame does not dispose anything', async () => {
    const m = await loadModule('linux', undefined)
    const v = new m.GstVideo({} as never)
    v.setCodecData(Buffer.from([7]))
    expect(gstHost.stop).not.toHaveBeenCalled()
    v.push('h264', Buffer.from([1]))
    expect(gstHost.createPlayer).toHaveBeenCalledWith(expect.any(Number), 'h264', Buffer.from([7]))
  })

  test('setCodecData while a claim is queued drops the claim', async () => {
    const m = await loadModule('linux', '/sock')
    const v1 = new m.GstVideo({} as never, 'main')
    const v2 = new m.GstVideo({} as never, 'dash')
    v1.push('h264', Buffer.from([1]))
    v2.push('h264', Buffer.from([2]))

    v2.setCodecData(Buffer.from([7]))
    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('bound main\n'))
    expect(gstHost.createPlayer).toHaveBeenCalledTimes(1)
  })

  test('setVisible toggles the compositor plane', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never, 'dash')
    v.setVisible(false)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('videoshow dash 0\n')
    v.setVisible(true)
    expect(sockets[0].write).toHaveBeenCalledWith('videoshow dash 1\n')
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

  test('setStreamGamma pushes the calibration to the compositor', async () => {
    const m = await loadModule('linux', '/sock')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    m.setStreamGamma(2, 1, 0.9, 0.8, 0.7)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('gamma 2 1 0.9 0.8 0.7\n')
  })
})

describe('GstVideo — darwin in-process addon path', () => {
  test('creates the addon player, starts it and pushes buffers', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))

    expect(addon.createPlayer).toHaveBeenCalledWith(
      'h264',
      Buffer.from([1, 2, 3, 4]),
      undefined,
      expect.any(Number)
    )
    expect(addon.start).toHaveBeenCalledTimes(1)
    expect(addon.setVisible).toHaveBeenCalledWith({ handle: 1 }, true)
    expect(addon.setGamma).toHaveBeenCalledWith({ handle: 1 }, 1, 1, 1, 1, 1)
    expect(addon.pushBuffer).toHaveBeenCalledTimes(1)

    v.push('h264', Buffer.from([2]))
    expect(addon.createPlayer).toHaveBeenCalledTimes(1)
    expect(addon.pushBuffer).toHaveBeenCalledTimes(2)
  })

  test('notifies the player-created hook after an addon create', async () => {
    const m = await loadModule('darwin')
    const created = vi.fn()
    m.setOnPlayerCreated(created)
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    expect(created).toHaveBeenCalledTimes(1)
    m.setOnPlayerCreated(null)
  })

  test('switching codecs recreates the in-process player', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    v.push('h265', Buffer.from([2]))
    expect(addon.stop).toHaveBeenCalledTimes(1)
    expect(addon.createPlayer).toHaveBeenCalledTimes(2)
  })

  test('prepare hands the codec_data record to the addon', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    const cd = Buffer.from([9, 9])
    v.prepare('h265', cd)
    expect(addon.createPlayer).toHaveBeenCalledWith(
      'h265',
      Buffer.from([1, 2, 3, 4]),
      cd,
      expect.any(Number)
    )
  })

  test('a preset content region is applied when the player is created', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.setContentRegion(10, 20, 800, 480, 1920, 1080)
    expect(addon.setContentRegion).not.toHaveBeenCalled()

    v.push('h264', Buffer.from([1]))
    expect(addon.setContentRegion).toHaveBeenCalledWith({ handle: 1 }, 10, 20, 800, 480, 1920, 1080)

    v.setContentRegion(0, 0, 0, 480, 1920, 1080)
    expect(addon.setContentRegion).toHaveBeenLastCalledWith({ handle: 1 }, 0, 0, 0, 0, 0, 0)
    v.setContentRegion(0, 0, 800, 0, 1920, 1080)
    expect(addon.setContentRegion).toHaveBeenLastCalledWith({ handle: 1 }, 0, 0, 0, 0, 0, 0)
  })

  test('no window handle means no player', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)

    fromWebContents.mockReturnValueOnce(null as never)
    v.push('h264', Buffer.from([1]))
    expect(addon.createPlayer).not.toHaveBeenCalled()
    expect(addon.pushBuffer).not.toHaveBeenCalled()

    win.isDestroyed.mockReturnValueOnce(true)
    v.push('h264', Buffer.from([1]))
    expect(addon.createPlayer).not.toHaveBeenCalled()
  })

  test('a null player from the addon is tolerated', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    addon.createPlayer.mockReturnValueOnce(null)
    v.push('h264', Buffer.from([1]))
    expect(addon.start).not.toHaveBeenCalled()
    expect(addon.pushBuffer).not.toHaveBeenCalled()
  })

  test('setVisible drives the addon only when a player exists', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    addon.setVisible.mockClear()

    v.setVisible(false)
    expect(addon.setVisible).toHaveBeenCalledWith({ handle: 1 }, false)

    const idle = new m.GstVideo({} as never)
    idle.setVisible(true)
    expect(addon.setVisible).toHaveBeenCalledTimes(1)
  })

  test('setStreamGamma reaches live addon players', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    new m.GstVideo({} as never)
    addon.setGamma.mockClear()

    m.setStreamGamma(2, 1, 1, 1, 1)
    expect(addon.setGamma).toHaveBeenCalledTimes(1)
    expect(addon.setGamma).toHaveBeenCalledWith({ handle: 1 }, 2, 1, 1, 1, 1)
  })

  test('dispose stops the addon player and survives a stop failure', async () => {
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    addon.stop.mockImplementationOnce(() => {
      throw new Error('teardown')
    })
    expect(() => v.dispose()).not.toThrow()
    v.dispose()
    expect(addon.stop).toHaveBeenCalledTimes(1)
  })

  test('everything is a no-op when the addon cannot load', async () => {
    loadState.fail = true
    const m = await loadModule('darwin')
    const v = new m.GstVideo({} as never)
    v.push('h264', Buffer.from([1]))
    v.prepare('h264')
    v.dispose()
    m.setStreamGamma(2, 1, 1, 1, 1)

    expect(addon.createPlayer).not.toHaveBeenCalled()
    expect(addon.setGamma).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalledWith(
      '[GstVideo] native addon load failed:',
      'addon unavailable'
    )
  })
})

describe('probeGstCodecs', () => {
  test('linux maps a complete host probe', async () => {
    probeViaHostMock.mockReturnValueOnce({
      h264: { hw: true, sw: false },
      h265: { hw: false, sw: true },
      vp9: { hw: false, sw: false },
      av1: { hw: true, sw: true }
    })
    const m = await loadModule('linux')

    expect(m.probeGstCodecs()).toEqual({
      h264: { hw: true, sw: false },
      h265: { hw: false, sw: true },
      vp9: { hw: false, sw: false },
      av1: { hw: true, sw: true }
    })
    expect(logSpy).toHaveBeenCalledWith('[GstVideo] GStreamer 1.24.5 (bundled: /gst)')
  })

  test('linux falls back to none when the host probe is unavailable', async () => {
    const m = await loadModule('linux')
    const none = { hw: false, sw: false }
    const empty = { h264: none, h265: none, vp9: none, av1: none }
    expect(m.probeGstCodecs()).toEqual(empty)
    expect(m.probeGstCodecs()).toEqual(empty)
  })

  test('linux falls back to none when the host probe is incomplete', async () => {
    const m = await loadModule('linux')
    const s = { hw: false, sw: true }
    const none = { hw: false, sw: false }
    const empty = { h264: none, h265: none, vp9: none, av1: none }
    probeViaHostMock.mockReturnValueOnce({})
    expect(m.probeGstCodecs()).toEqual(empty)
    probeViaHostMock.mockReturnValueOnce({ h264: s })
    expect(m.probeGstCodecs()).toEqual(empty)
    probeViaHostMock.mockReturnValueOnce({ h264: s, h265: s })
    expect(m.probeGstCodecs()).toEqual(empty)
    probeViaHostMock.mockReturnValueOnce({ h264: s, h265: s, vp9: s })
    expect(m.probeGstCodecs()).toEqual(empty)
  })

  test('darwin probes via the in-process addon', async () => {
    const m = await loadModule('darwin')
    expect(m.probeGstCodecs()).toEqual({
      h264: { hw: true, sw: true },
      h265: { hw: false, sw: true },
      vp9: { hw: false, sw: true },
      av1: { hw: false, sw: true }
    })
    expect(probeViaHostMock).not.toHaveBeenCalled()
    expect(m.probeGstCodecs().h264.hw).toBe(true)
    expect(addon.version).toHaveBeenCalledTimes(1)
  })

  test('darwin returns none when the addon probe throws', async () => {
    const m = await loadModule('darwin')
    addon.probeCodecs.mockImplementationOnce(() => {
      throw new Error('probe crash')
    })
    const none = { hw: false, sw: false }
    expect(m.probeGstCodecs()).toEqual({ h264: none, h265: none, vp9: none, av1: none })
  })

  test('darwin returns none when the addon cannot load', async () => {
    loadState.fail = true
    const m = await loadModule('darwin')
    const none = { hw: false, sw: false }
    expect(m.probeGstCodecs()).toEqual({ h264: none, h265: none, vp9: none, av1: none })
  })
})

describe('GStreamer runtime logging', () => {
  test('a failing bundle lookup reads as no bundle', async () => {
    resolveRootMock.mockImplementationOnce(() => {
      throw new Error('lookup failed')
    })
    const m = await loadModule('linux')
    m.probeGstCodecs()
    expect(warnSpy).toHaveBeenCalledWith(
      `[GstVideo] no bundled GStreamer for linux-${process.arch}, using the system install`
    )
  })

  test('a missing gst-launch binary reads as no bundle', async () => {
    resolveBinaryMock.mockReturnValueOnce(null)
    const m = await loadModule('linux')
    m.probeGstCodecs()
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('no bundled GStreamer'))
  })

  test('unparseable version output warns', async () => {
    execFileSyncMock.mockReturnValueOnce('not a version banner')
    const m = await loadModule('linux')
    m.probeGstCodecs()
    expect(warnSpy).toHaveBeenCalledWith(
      '[GstVideo] bundled GStreamer at /gst reported no version: not a version banner'
    )
  })

  test('a failing gst-launch run warns', async () => {
    execFileSyncMock.mockImplementationOnce(() => {
      throw new Error('exec denied')
    })
    const m = await loadModule('linux')
    m.probeGstCodecs()
    expect(warnSpy).toHaveBeenCalledWith(
      '[GstVideo] bundled GStreamer at /gst could not be run: exec denied'
    )
  })

  test('darwin without a bundle names the addon version', async () => {
    resolveRootMock.mockReturnValueOnce(null)
    const m = await loadModule('darwin')
    m.probeGstCodecs()
    expect(warnSpy).toHaveBeenCalledWith(
      `[GstVideo] no bundled GStreamer for darwin-${process.arch}, using the system install 1.0-test`
    )
  })

  test('a packaged mac build points GStreamer at the bundle', async () => {
    electronApp.isPackaged = true
    const m = await loadModule('darwin')
    m.probeGstCodecs()
    expect(process.env.GST_PLUGIN_SYSTEM_PATH).toBe('')
    expect(process.env.GST_PLUGIN_PATH).toBe('/gst/lib/gstreamer-1.0')
    expect(process.env.GST_PLUGIN_SCANNER).toBe('/gst/libexec/gstreamer-1.0/gst-plugin-scanner')
  })

  test('a packaged mac build without a bundle leaves the env alone', async () => {
    electronApp.isPackaged = true
    resolveRootMock.mockReturnValue(null)
    const m = await loadModule('darwin')
    m.probeGstCodecs()
    expect(process.env.GST_PLUGIN_PATH).toBeUndefined()
  })
})

describe('setMacBackdrop', () => {
  test('paints the mac window content view', async () => {
    const m = await loadModule('darwin')
    m.setMacBackdrop(win as never, '#ff8000')
    expect(addon.setBackdrop).toHaveBeenCalledWith(Buffer.from([1, 2, 3, 4]), 1, 128 / 255, 0)
  })

  test('ignores a failing native call', async () => {
    const m = await loadModule('darwin')
    ;(addon.setBackdrop as ReturnType<typeof vi.fn>).mockImplementationOnce(() => {
      throw new Error('no handle yet')
    })
    expect(() => m.setMacBackdrop(win as never, '#ffffff')).not.toThrow()
  })

  test('skips missing or destroyed windows', async () => {
    const m = await loadModule('darwin')
    m.setMacBackdrop(null as never, '#ffffff')
    win.isDestroyed.mockReturnValueOnce(true)
    m.setMacBackdrop(win as never, '#ffffff')
    expect(addon.setBackdrop).not.toHaveBeenCalled()
  })

  test('skips when the addon lacks setBackdrop', async () => {
    const m = await loadModule('darwin')
    const original = addon.setBackdrop
    addon.setBackdrop = undefined
    expect(() => m.setMacBackdrop(win as never, '#ffffff')).not.toThrow()
    addon.setBackdrop = original
  })

  test('skips when the addon failed to load', async () => {
    loadState.fail = true
    const m = await loadModule('darwin')
    expect(() => m.setMacBackdrop(win as never, '#ffffff')).not.toThrow()
    expect(addon.setBackdrop).not.toHaveBeenCalled()
  })

  test('is a no-op off darwin', async () => {
    const m = await loadModule('linux')
    const fakeWin = { isDestroyed: () => false, getNativeWindowHandle: () => Buffer.from([1]) }
    expect(() => m.setMacBackdrop(fakeWin as never, '#ffffff')).not.toThrow()
  })
})

describe('backdrop helpers', () => {
  test('backdropHex resolves dark/light with theme fallbacks', async () => {
    const m = await loadModule()
    expect(m.backdropHex(true, '#111111', '#eeeeee')).toBe('#111111')
    expect(m.backdropHex(false, '#111111', '#eeeeee')).toBe('#eeeeee')
    expect(m.backdropHex(true)).toBe('#000000')
    expect(m.backdropHex(false)).toBe('#d4d4d4')
  })
})

describe('media feed', () => {
  test('on linux the media feed comes from the host process', async () => {
    const mod = await loadModule('linux')
    expect(mod.useHostProcess).toBe(true)
    await expect(mod.openMediaFeed()).resolves.toBe('/host/feed')
    expect(gstHost.openFeed).toHaveBeenCalled()
    expect(mod.gstAddon()).toBeNull()
  })

  test('on darwin the addon binds the feed once and reuses it', async () => {
    const mod = await loadModule('darwin')
    expect(mod.useHostProcess).toBe(false)
    const first = await mod.openMediaFeed()
    expect(first).toMatch(/livi-gst-\d+\.feed$/)
    expect(addon.openFeed).toHaveBeenCalledWith(first)
    ;(addon.openFeed as ReturnType<typeof vi.fn>).mockClear()
    await expect(mod.openMediaFeed()).resolves.toBe(first)
    expect(addon.openFeed).not.toHaveBeenCalled()
    expect(mod.gstAddon()).toBe(addon)
  })

  test('on darwin an unbound addon feed is empty', async () => {
    ;(addon.openFeed as ReturnType<typeof vi.fn>).mockReturnValueOnce(false)
    const mod = await loadModule('darwin')
    await expect(mod.openMediaFeed()).resolves.toBe('')
  })
})
