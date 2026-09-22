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
  getPosition = vi.fn(() => [100, 100])
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
  screen: { getAllDisplays: vi.fn(() => []) }
}))

const { setCompositorScreenMock, setMacBackdropMock } = vi.hoisted(() => ({
  setCompositorScreenMock: vi.fn(),
  setMacBackdropMock: vi.fn()
}))
vi.mock('@main/services/video/GstVideo', () => ({
  backdropHex: vi.fn(() => '#000000'),
  setMacBackdrop: setMacBackdropMock,
  setCompositorScreen: setCompositorScreenMock
}))

const { configEvents } = vi.hoisted(() => ({
  configEvents: { on: vi.fn(), off: vi.fn(), emit: vi.fn() }
}))
const saveSettingsMock = vi.fn()
vi.mock('@main/ipc/utils', () => ({
  configEvents,
  saveSettings: (...a: unknown[]) => saveSettingsMock(...a)
}))

import { COMPOSITOR_TITLEBAR_H } from '@main/app/compositorLayout'
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
      dashScreenActive: false,
      auxScreenActive: false,
      dashScreenWidth: 800,
      dashScreenHeight: 480,
      auxScreenWidth: 1024,
      auxScreenHeight: 600,
      ...over
    },
    isQuitting: false
  } as runtimeStateProps
}

beforeEach(() => {
  lastWindows.length = 0
  saveSettingsMock.mockReset()
  configEvents.on.mockReset()
  setCompositorScreenMock.mockClear()
  setMacBackdropMock.mockClear()
  vi.useFakeTimers()
  closeAllSecondaryWindows()
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('syncSecondaryWindows — open / close', () => {
  test('spawns a window when active flag is set', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(1)
    expect(getSecondaryWindow('dash')).not.toBeNull()
  })

  test('closes the window when active flag is cleared', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.dashScreenActive = false
    syncSecondaryWindows(rt, { dashScreenActive: true } as never)
    expect(win.close).toHaveBeenCalled()
  })

  test('isQuitting short-circuits the sync', () => {
    const rt = baseState({ dashScreenActive: true })
    rt.isQuitting = true
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(0)
  })

  test('no-op when window is already open and config unchanged', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(1)
    syncSecondaryWindows(rt, { dashScreenActive: true } as never)
    expect(lastWindows).toHaveLength(1)
  })
})

describe('syncSecondaryWindows — resize + kiosk', () => {
  test('resizes the window when width/height change', () => {
    const rt = baseState({ dashScreenActive: true, dashScreenWidth: 800, dashScreenHeight: 480 })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.dashScreenWidth = 1024
    rt.config.dashScreenHeight = 600
    syncSecondaryWindows(rt, {
      dashScreenActive: true,
      dashScreenWidth: 800,
      dashScreenHeight: 480
    } as never)
    expect(win.setContentSize).toHaveBeenCalledWith(1024, 600)
  })

  test('applyKiosk toggles fullScreen on darwin', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.kiosk = { dash: true } as never
    syncSecondaryWindows(rt, { dashScreenActive: true, kiosk: { dash: false } } as never)
    expect(win.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('applyKiosk toggles kiosk on linux', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.kiosk = { dash: true } as never
    syncSecondaryWindows(rt, { dashScreenActive: true, kiosk: { dash: false } } as never)
    expect(win.setKiosk).toHaveBeenCalledWith(true)
  })
})

describe('window lifecycle', () => {
  test('window emits move/resize → schedule save', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('move')
    win.emit('resize')
    vi.advanceTimersByTime(500)
    expect(saveSettingsMock).toHaveBeenCalled()
  })

  test('window "closed" event clears the active flag', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('closed')
    expect(saveSettingsMock).toHaveBeenCalledWith(rt, { dashScreenActive: false })
  })

  test('"closed" while quitting does not save', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.isQuitting = true
    win.emit('closed')
    expect(saveSettingsMock).not.toHaveBeenCalled()
  })
})

describe('setupSecondaryWindows + closeAllSecondaryWindows', () => {
  test('setup runs initial sync + subscribes to config changes', () => {
    const rt = baseState({ dashScreenActive: true })
    setupSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(1)
    expect(configEvents.on).toHaveBeenCalledWith('changed', expect.any(Function))
  })

  test('config "changed" callback re-syncs', () => {
    const rt = baseState({ dashScreenActive: false })
    setupSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(0)
    const cb = configEvents.on.mock.calls.find((c) => c[0] === 'changed')![1] as (
      next: unknown,
      prev: unknown
    ) => void
    rt.config.dashScreenActive = true
    cb(rt.config, { dashScreenActive: false })
    expect(lastWindows).toHaveLength(1)
  })

  test('closeAllSecondaryWindows closes every open window', () => {
    const rt = baseState({ dashScreenActive: true, auxScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows).toHaveLength(2)
    closeAllSecondaryWindows()
    for (const w of lastWindows) expect(w.close).toHaveBeenCalled()
  })

  test('getSecondaryWindow returns null for an unopened role', () => {
    expect(getSecondaryWindow('aux')).toBeNull()
  })

  test('getSecondaryWindow returns the open window for a role', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    expect(getSecondaryWindow('dash')).not.toBeNull()
  })
})

describe('secondaryWindows — bounds + ready-to-show', () => {
  test('bounds from config are applied on ready-to-show', () => {
    const rt = baseState({
      dashScreenActive: true,
      dashScreenBounds: { x: 10, y: 20, width: 1024, height: 768 }
    } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    // width/height restored as content size, position separately (no titlebar drift)
    expect(win.setContentSize).toHaveBeenCalledWith(1024, 768)
    expect(win.setPosition).toHaveBeenCalledWith(10, 20)
  })

  test('darwin kiosk applies on ready-to-show', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: true } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    expect(win.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('linux kiosk applies on ready-to-show', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: true } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    expect(win.setKiosk).toHaveBeenCalledWith(true)
  })

  test('persistBounds skips when fullScreen / kiosk', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isFullScreen.mockReturnValue(true)
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveSettingsMock).not.toHaveBeenCalled()
  })

  test('persistBounds skips when destroyed', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveSettingsMock).not.toHaveBeenCalled()
  })

  test('persistBounds skips when bounds unchanged', () => {
    const rt = baseState({
      dashScreenActive: true,
      dashScreenBounds: { x: 100, y: 100, width: 800, height: 480 }
    } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveSettingsMock).not.toHaveBeenCalled()
  })

  test('resize is a no-op when fullScreen / kiosk', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isFullScreen.mockReturnValue(true)
    rt.config.dashScreenWidth = 1234
    syncSecondaryWindows(rt, {
      dashScreenActive: true,
      dashScreenWidth: 800,
      dashScreenHeight: 480
    } as never)
    expect(win.setContentSize).not.toHaveBeenCalled()
  })

  test('spawn falls back to default sizes for invalid config values', () => {
    const rt = baseState({ dashScreenActive: true, dashScreenWidth: 0, dashScreenHeight: 0 })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.width).toBe(800)
    expect(win.__opts.height).toBe(480)
  })

  test('spawn bounds an oversized config value to the maximum', () => {
    const rt = baseState({
      dashScreenActive: true,
      dashScreenWidth: 2213123132132,
      dashScreenHeight: 999999
    })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.width).toBe(4096)
    expect(win.__opts.height).toBe(2160)
  })

  test('spawn lifts an undersized config value to the minimum', () => {
    const rt = baseState({ dashScreenActive: true, dashScreenWidth: 12, dashScreenHeight: 7 })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.width).toBe(300)
    expect(win.__opts.height).toBe(200)
  })

  test('a loaded window announces itself as ready', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    const ready = vi.fn()
    secondaryWindowEvents.once('ready', ready)

    const [event, handler] = win.webContents.once.mock.calls[0]
    expect(event).toBe('did-finish-load')
    handler()

    expect(ready).toHaveBeenCalledWith('dash')
  })

  test('spawned window denies popups and filters permissions', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    const openHandler = win.webContents.setWindowOpenHandler.mock.calls[0][0]
    const result = openHandler({ url: 'https://example.com' })
    expect(shell.openExternal).toHaveBeenCalledWith('https://example.com')
    expect(result).toEqual({ action: 'deny' })
    const check = win.webContents.session.setPermissionCheckHandler.mock.calls[0][0]
    expect(check({}, 'usb')).toBe(true)
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
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    expect(lastWindows[0].loadURL).toHaveBeenCalledWith('http://localhost:5173?role=dash')
    ;(app as { isPackaged: boolean }).isPackaged = true
    if (original === undefined) delete process.env.ELECTRON_RENDERER_URL
    else process.env.ELECTRON_RENDERER_URL = original
  })

  test('ready-to-show handlers bail out on a destroyed window', () => {
    const rt = baseState({
      dashScreenActive: true,
      dashScreenBounds: { x: 1, y: 2, width: 640, height: 400 },
      kiosk: { dash: true }
    } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    win.emit('ready-to-show')
    expect(win.setContentSize).not.toHaveBeenCalled()
    expect(win.setPosition).not.toHaveBeenCalled()
    expect(win.setFullScreen).not.toHaveBeenCalled()
    expect(win.setKiosk).not.toHaveBeenCalled()
    expect(setMacBackdropMock).not.toHaveBeenCalled()
  })

  test('ready-to-show paints the mac backdrop', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('ready-to-show')
    expect(setMacBackdropMock).toHaveBeenCalledWith(win, '#000000')
  })

  test('closed clears a pending bounds timer and skips save when flag already false', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.emit('move')
    rt.config.dashScreenActive = false
    win.emit('closed')
    vi.advanceTimersByTime(500)
    expect(saveSettingsMock).not.toHaveBeenCalled()
  })

  test('resize is a no-op when the window is destroyed', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    rt.config.dashScreenWidth = 1024
    syncSecondaryWindows(rt, {
      dashScreenActive: true,
      dashScreenWidth: 800,
      dashScreenHeight: 480
    } as never)
    expect(win.setContentSize).not.toHaveBeenCalled()
  })

  test('a height-only change still triggers a resize', () => {
    const rt = baseState({ dashScreenActive: true })
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    rt.config.dashScreenHeight = 600
    syncSecondaryWindows(rt, {
      dashScreenActive: true,
      dashScreenWidth: 800,
      dashScreenHeight: 480
    } as never)
    expect(win.setContentSize).toHaveBeenCalledWith(800, 600)
  })

  test('applyKiosk is a no-op when the window is destroyed', () => {
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isDestroyed.mockReturnValue(true)
    rt.config.kiosk = { dash: true } as never
    syncSecondaryWindows(rt, { dashScreenActive: true, kiosk: { dash: false } } as never)
    expect(win.setFullScreen).not.toHaveBeenCalled()
    expect(win.setKiosk).not.toHaveBeenCalled()
  })

  test('applyKiosk skips when darwin fullscreen already matches', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isFullScreen.mockReturnValue(true)
    rt.config.kiosk = { dash: true } as never
    syncSecondaryWindows(rt, { dashScreenActive: true, kiosk: { dash: false } } as never)
    expect(win.setFullScreen).not.toHaveBeenCalled()
  })

  test('applyKiosk skips when linux kiosk already matches', () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const rt = baseState({ dashScreenActive: true, kiosk: { dash: false } } as never)
    syncSecondaryWindows(rt)
    const win = lastWindows[0]
    win.isKiosk.mockReturnValue(true)
    rt.config.kiosk = { dash: true } as never
    syncSecondaryWindows(rt, {
      dashScreenActive: true,
      dashScreenWidth: 800,
      dashScreenHeight: 480,
      kiosk: { dash: false }
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
      dashScreenActive: true,
      dashScreenBounds: { x: 1, y: 2, width: 640, height: 400 }
    } as never)
    mod.syncSecondaryWindows(rt)
    const win = lastWindows[0]
    expect(win.__opts.title).toBe('livi:dash')
    expect(win.__opts.x).toBeUndefined()
    expect(win.__opts.transparent).toBe(true)
    win.emit('move')
    vi.advanceTimersByTime(500)
    expect(saveSettingsMock).not.toHaveBeenCalled()
    mod.closeAllSecondaryWindows()
  })

  test('size change reopens the compositor output', async () => {
    const mod = await loadCompositor()
    const rt = baseState({ dashScreenActive: true })
    mod.syncSecondaryWindows(rt)
    const win = lastWindows[0]
    setCompositorScreenMock.mockClear()
    rt.config.dashScreenWidth = 1024
    mod.syncSecondaryWindows(rt, {
      dashScreenActive: true,
      dashScreenWidth: 800,
      dashScreenHeight: 480
    } as never)
    expect(win.setContentSize).toHaveBeenCalledWith(1024, 480)
    expect(setCompositorScreenMock).toHaveBeenCalledWith('dash', false)
    expect(setCompositorScreenMock).toHaveBeenCalledWith(
      'dash',
      true,
      1024,
      480 + COMPOSITOR_TITLEBAR_H
    )
    mod.closeAllSecondaryWindows()
  })
})
