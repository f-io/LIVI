import { EventEmitter } from 'node:events'

class MockSession {
  setPermissionCheckHandler = vi.fn()
  setPermissionRequestHandler = vi.fn()
}

class MockWebContents {
  session = new MockSession()
  setWindowOpenHandler = vi.fn()
  once = vi.fn()
}

class MockBrowserWindow extends EventEmitter {
  __opts: Record<string, unknown> = {}
  webContents = new MockWebContents()
  loadURL = vi.fn()
  close = vi.fn()
  setBounds = vi.fn()
  setContentSize = vi.fn()
  setPosition = vi.fn()
  setFullScreen = vi.fn()
  setKiosk = vi.fn()
  setAspectRatio = vi.fn()
  setMinimumSize = vi.fn()
  setResizable = vi.fn()
  getBounds = vi.fn(() => ({ x: 100, y: 100, width: 800, height: 508 }))
  getPosition = vi.fn(() => [100, 100])
  getSize = vi.fn(() => [800, 508])
  getContentSize = vi.fn(() => [800, 480])
  isDestroyed = vi.fn(() => false)
  isFullScreen = vi.fn(() => false)
  isKiosk = vi.fn(() => false)
  once(event: string, listener: (...args: unknown[]) => void): this {
    super.once(event, listener)
    return this
  }
}

const lastWindows: MockBrowserWindow[] = []

vi.mock('electron', () => ({
  app: { isPackaged: true },
  BrowserWindow: vi.fn().mockImplementation(function (opts: Record<string, unknown>) {
    const w = new MockBrowserWindow()
    w.__opts = opts
    lastWindows.push(w)
    return w
  }),
  shell: { openExternal: vi.fn() },
  screen: {
    getAllDisplays: vi.fn(() => []),
    getDisplayMatching: vi.fn(() => ({ workAreaSize: { width: 3000, height: 2000 } }))
  }
}))

const { configEvents } = vi.hoisted(() => ({
  configEvents: { on: vi.fn(), off: vi.fn(), emit: vi.fn() }
}))
const saveConfigMock = vi.fn()
vi.mock('@main/window/applyConfig', () => ({ configEvents }))
const drawVideoIn = vi.fn()
vi.mock('@main/video', () => ({ drawVideoIn: (...a: unknown[]) => drawVideoIn(...a) }))
vi.mock('@main/core', () => ({
  saveConfig: (...a: unknown[]) => saveConfigMock(...a)
}))

import type { runtimeStateProps } from '@main/types'
import { app, shell } from 'electron'
import {
  closeAllSecondaryWindows,
  getSecondaryWindow,
  secondaryWindowEvents,
  setupSecondaryWindows,
  syncSecondaryWindows
} from '../secondaryWindows'

function baseState(over: Partial<runtimeStateProps['config']> = {}): runtimeStateProps {
  return {
    config: {
      clusterScreenActive: false,
      auxScreenActive: false,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480,
      auxScreenWidth: 1024,
      auxScreenHeight: 600,
      ...over
    },
    isQuitting: false
  } as runtimeStateProps
}

beforeEach(() => {
  lastWindows.length = 0
  saveConfigMock.mockReset()
  configEvents.on.mockReset()
  vi.useFakeTimers()
  closeAllSecondaryWindows()
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('syncSecondaryWindows — open / close', () => {
  test('spawns a window when active flag is set', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(1)
    expect(getSecondaryWindow('cluster')).not.toBeNull()
  })

  test('closes the window when active flag is cleared', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.clusterScreenActive = false
    syncSecondaryWindows(rt, { clusterScreenActive: true } as never)
    expect(win.close).toHaveBeenCalled()
  })

  test('isQuitting short-circuits the sync', () => {
    const rt = baseState({ clusterScreenActive: true })
    rt.isQuitting = true
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(0)
  })

  test('no-op when window is already open and config unchanged', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(1)
    syncSecondaryWindows(rt, { clusterScreenActive: true } as never)
    expect(lastWindows).toHaveLength(1)
  })
})

describe('syncSecondaryWindows — resize + kiosk', () => {
  test('resizes the window when width/height change', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480
    })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.clusterScreenWidth = 1024
    rt.config.clusterScreenHeight = 600
    syncSecondaryWindows(rt, {
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480
    } as never)
    expect(win.setContentSize).toHaveBeenCalledWith(1024, 600, false)
    expect(win.setAspectRatio).toHaveBeenCalledWith(1024 / 600, { width: 0, height: 0 })
  })

  test('applyKiosk toggles fullScreen on darwin', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.kiosk = { cluster: true } as never
    syncSecondaryWindows(rt, { clusterScreenActive: true, kiosk: { cluster: false } } as never)
    expect(win.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('applyKiosk toggles kiosk on linux', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.kiosk = { cluster: true } as never
    syncSecondaryWindows(rt, { clusterScreenActive: true, kiosk: { cluster: false } } as never)
    expect(win.setKiosk).toHaveBeenCalledWith(true)
  })
})

describe('window lifecycle', () => {
  test('window emits move/resize → schedule save', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('move')
    win.emit('resize')
    vi.advanceTimersByTime(500)
    expect(saveConfigMock).toHaveBeenCalled()
  })

  test('window "closed" event clears the active flag', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(drawVideoIn).toHaveBeenCalledWith('cluster', win)
    win.emit('closed')
    expect(drawVideoIn).toHaveBeenLastCalledWith('cluster', null)
    expect(saveConfigMock).toHaveBeenCalledWith({ clusterScreenActive: false })
  })

  test('"closed" while quitting does not save', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.isQuitting = true
    win.emit('closed')
    expect(saveConfigMock).not.toHaveBeenCalled()
  })
})

describe('setupSecondaryWindows + closeAllSecondaryWindows', () => {
  test('setup runs initial sync + subscribes to config changes', () => {
    const rt = baseState({ clusterScreenActive: true })
    setupSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(1)
    expect(configEvents.on).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('config "changed" callback re-syncs', () => {
    const rt = baseState({ clusterScreenActive: false })
    setupSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(0)
    const cb = configEvents.on.mock.calls.find((c) => c[0] === 'changed')![1] as (
      next: unknown,
      prev: unknown
    ) => void
    rt.config.clusterScreenActive = true
    cb(rt.config, { clusterScreenActive: false })
    expect(lastWindows).toHaveLength(1)
  })

  test('closeAllSecondaryWindows closes every open window', () => {
    const rt = baseState({ clusterScreenActive: true, auxScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(2)
    closeAllSecondaryWindows()
    for (const w of lastWindows) expect(w.close).toHaveBeenCalled()
  })

  test('getSecondaryWindow returns null for an unopened role', () => {
    expect(getSecondaryWindow('aux')).toBeNull()
  })

  test('getSecondaryWindow returns the open window for a role', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    expect(getSecondaryWindow('cluster')).not.toBeNull()
  })
})

describe('secondaryWindows — bounds + ready-to-show', () => {
  test('a saved window keeps its place and width in the configured shape', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenBounds: { x: 10, y: 20, width: 1024, height: 768 }
    } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    // Separate calls keep the titlebar from shifting the window.
    expect(win.setContentSize).toHaveBeenCalledWith(1024, 614, false)
    expect(win.setPosition).toHaveBeenCalledWith(10, 20)
  })

  test('without a saved window it opens at the configured size', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    expect(win.setContentSize).toHaveBeenCalledWith(800, 480, false)
    expect(win.setPosition).not.toHaveBeenCalled()
  })

  test('darwin kiosk applies on ready-to-show', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: true } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    expect(win.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('linux kiosk applies on ready-to-show', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: true } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    expect(win.setKiosk).toHaveBeenCalledWith(true)
  })

  test('persistBounds skips when fullScreen / kiosk', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isFullScreen.mockReturnValue(true)
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveConfigMock).not.toHaveBeenCalled()
  })

  test('persistBounds skips when destroyed', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveConfigMock).not.toHaveBeenCalled()
  })

  test('persistBounds skips when bounds unchanged', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenBounds: { x: 100, y: 100, width: 800, height: 480 }
    } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveConfigMock).not.toHaveBeenCalled()
  })

  test('resize is a no-op when fullScreen / kiosk', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isFullScreen.mockReturnValue(true)
    rt.config.clusterScreenWidth = 1234
    syncSecondaryWindows(rt, {
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480
    } as never)
    expect(win.setContentSize).not.toHaveBeenCalled()
  })

  test('spawn falls back to default sizes for invalid config values', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenWidth: 0,
      clusterScreenHeight: 0
    })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.width).toBe(800)
    expect(win.__opts.height).toBe(480)
  })

  test('spawn bounds an oversized config value to the maximum', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenWidth: 2213123132132,
      clusterScreenHeight: 999999
    })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.width).toBe(4096)
    expect(win.__opts.height).toBe(2160)
  })

  test('spawn lifts an undersized config value to the minimum', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenWidth: 12,
      clusterScreenHeight: 7
    })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.width).toBe(300)
    expect(win.__opts.height).toBe(200)
  })

  test('a loaded window announces itself as ready', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    const ready = vi.fn()
    secondaryWindowEvents.once('ready', ready)

    const [event, handler] = win.webContents.once.mock.calls[0]
    expect(event).toBe('did-finish-load')
    handler()

    expect(ready).toHaveBeenCalledWith('cluster')
  })

  test('spawned window denies popups and filters permissions', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    const openHandler = win.webContents.setWindowOpenHandler.mock.calls[0][0]
    const result = openHandler({ url: 'https://example.com' })
    expect(shell.openExternal).toHaveBeenCalledWith('https://example.com')
    expect(result).toEqual({ action: 'deny' })
    const check = win.webContents.session.setPermissionCheckHandler.mock.calls[0][0]
    expect(check({}, 'media')).toBe(true)
    expect(check({}, 'usb')).toBe(false)
    expect(check({}, 'geolocation')).toBe(false)
    const request = win.webContents.session.setPermissionRequestHandler.mock.calls[0][0]
    const cb = vi.fn()
    request({}, 'media', cb)
    expect(cb).toHaveBeenCalledWith(true)
  })

  test('dev mode loads the renderer url with a role query', () => {
    const original = process.env.ELECTRON_RENDERER_URL
    ;(app as { isPackaged: boolean }).isPackaged = false
    process.env.ELECTRON_RENDERER_URL = 'http://localhost:5173'
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows[0].loadURL).toHaveBeenCalledWith('http://localhost:5173?role=cluster')
    ;(app as { isPackaged: boolean }).isPackaged = true
    if (original === undefined) delete process.env.ELECTRON_RENDERER_URL
    else process.env.ELECTRON_RENDERER_URL = original
  })

  test('ready-to-show handlers bail out on a destroyed window', () => {
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenBounds: { x: 1, y: 2, width: 640, height: 400 },
      kiosk: { cluster: true }
    } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    win.emit('ready-to-show')
    expect(win.setContentSize).not.toHaveBeenCalled()
    expect(win.setPosition).not.toHaveBeenCalled()
    expect(win.setFullScreen).not.toHaveBeenCalled()
    expect(win.setKiosk).not.toHaveBeenCalled()
  })

  test('closed clears a pending bounds timer and skips save when flag already false', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('move')
    rt.config.clusterScreenActive = false
    win.emit('closed')
    vi.advanceTimersByTime(500)
    expect(saveConfigMock).not.toHaveBeenCalled()
  })

  test('resize is a no-op when the window is destroyed', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    rt.config.clusterScreenWidth = 1024
    syncSecondaryWindows(rt, {
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480
    } as never)
    expect(win.setContentSize).not.toHaveBeenCalled()
  })

  test('a height-only change still triggers a resize', () => {
    const rt = baseState({ clusterScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.clusterScreenHeight = 600
    syncSecondaryWindows(rt, {
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480
    } as never)
    expect(win.setContentSize).toHaveBeenCalledWith(800, 600, false)
  })

  test('applyKiosk is a no-op when the window is destroyed', () => {
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    rt.config.kiosk = { cluster: true } as never
    syncSecondaryWindows(rt, { clusterScreenActive: true, kiosk: { cluster: false } } as never)
    expect(win.setFullScreen).not.toHaveBeenCalled()
    expect(win.setKiosk).not.toHaveBeenCalled()
  })

  test('applyKiosk skips when darwin fullscreen already matches', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isFullScreen.mockReturnValue(true)
    rt.config.kiosk = { cluster: true } as never
    syncSecondaryWindows(rt, { clusterScreenActive: true, kiosk: { cluster: false } } as never)
    expect(win.setFullScreen).not.toHaveBeenCalled()
  })

  test('applyKiosk skips when linux kiosk already matches', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const rt = baseState({ clusterScreenActive: true, kiosk: { cluster: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isKiosk.mockReturnValue(true)
    rt.config.kiosk = { cluster: true } as never
    syncSecondaryWindows(rt, {
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480,
      kiosk: { cluster: false }
    } as never)
    expect(win.setKiosk).not.toHaveBeenCalled()
    expect(win.setContentSize).not.toHaveBeenCalled()
  })
})

describe('secondaryWindows — compositor mode', () => {
  const loadCompositor = async () => {
    vi.resetModules()
    process.env.LIVI_COMPOSITOR = '1'
    return await import('../secondaryWindows')
  }

  afterEach(() => {
    delete process.env.LIVI_COMPOSITOR
  })

  test('spawn ignores saved bounds, titles by role and skips bounds persistence', async () => {
    const mod = await loadCompositor()
    const rt = baseState({
      clusterScreenActive: true,
      clusterScreenBounds: { x: 1, y: 2, width: 640, height: 400 }
    } as never)
    mod.syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.title).toBe('livi:cluster')
    expect(win.__opts.x).toBeUndefined()
    expect(win.__opts.transparent).toBe(true)
    win.emit('ready-to-show')
    expect(win.setContentSize).not.toHaveBeenCalled()
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveConfigMock).not.toHaveBeenCalled()
    mod.closeAllSecondaryWindows()
  })

  test('a size change resizes the window', async () => {
    const mod = await loadCompositor()
    const rt = baseState({ clusterScreenActive: true })
    mod.syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.clusterScreenWidth = 1024
    mod.syncSecondaryWindows(rt, {
      clusterScreenActive: true,
      clusterScreenWidth: 800,
      clusterScreenHeight: 480
    } as never)
    expect(win.setContentSize).toHaveBeenCalledWith(1024, 480, false)
    mod.closeAllSecondaryWindows()
  })
})
