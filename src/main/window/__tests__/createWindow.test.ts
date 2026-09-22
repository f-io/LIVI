import { isDev, isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import { createMainWindow, getMainWindow } from '@main/window/createWindow'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  attachKioskStateSync,
  persistKioskAndBroadcast
} from '@main/window/utils'
import { screen, session, shell } from 'electron'
import type { Mock } from 'vitest'

const browserWindowInstances: any[] = []

vi.mock('electron', async () => {
  const BrowserWindow = vi.fn(function (opts) {
    const instance = {
      __opts: opts,
      webContents: {
        session: {
          setPermissionCheckHandler: vi.fn(),
          setPermissionRequestHandler: vi.fn(),
          setUSBProtectedClassesHandler: vi.fn()
        },
        setWindowOpenHandler: vi.fn(),
        setZoomFactor: vi.fn(),
        openDevTools: vi.fn(),
        on: vi.fn()
      },
      once: vi.fn(),
      on: vi.fn(),
      loadURL: vi.fn(),
      setKiosk: vi.fn(),
      setContentSize: vi.fn(),
      getContentSize: vi.fn(() => [800, 480]),
      show: vi.fn(),
      hide: vi.fn(),
      getBounds: vi.fn(function () {
        return { x: 0, y: 0, width: 800, height: 480 }
      }),
      isDestroyed: vi.fn(() => false),
      isFullScreen: vi.fn(() => false),
      setFullScreen: vi.fn()
    }
    browserWindowInstances.push(instance)
    return instance
  })

  return {
    app: {
      quit: vi.fn(),
      getPath: vi.fn(() => '/tmp')
    },
    BrowserWindow: Object.assign(BrowserWindow, {
      getAllWindows: vi.fn(() => [])
    }),
    session: {
      defaultSession: { webRequest: { onHeadersReceived: vi.fn() } }
    },
    shell: {
      openExternal: vi.fn()
    },
    screen: {
      getDisplayMatching: vi.fn(function () {
        return {
          size: { width: 1920, height: 1080 },
          workAreaSize: { width: 1920, height: 1080 }
        }
      })
    }
  }
})

vi.mock('@main/utils', () => ({
  isDev: vi.fn(() => false),
  isMacPlatform: vi.fn(() => false),
  pushSettingsToRenderer: vi.fn()
}))

vi.mock('@main/window/utils', () => ({
  applyAspectRatioFullscreen: vi.fn(),
  applyAspectRatioWindowed: vi.fn(),
  applyWindowedContentSize: vi.fn(),
  attachKioskStateSync: vi.fn(),
  attachResizeReflow: vi.fn(),
  currentKiosk: vi.fn(() => false),
  persistKioskAndBroadcast: vi.fn(),
  sanitizeBounds: vi.fn((b) => b),
  uiZoomFactor: vi.fn((pct?: number) => (pct ?? 100) / 100)
}))

vi.mock('@main/ipc/utils', () => ({
  saveSettings: vi.fn()
}))

describe('createMainWindow', () => {
  const originalRendererUrl = process.env.ELECTRON_RENDERER_URL

  beforeEach(async () => {
    browserWindowInstances.length = 0
    vi.clearAllMocks()
    process.env.ELECTRON_RENDERER_URL = originalRendererUrl
  })

  afterAll(async () => {
    process.env.ELECTRON_RENDERER_URL = originalRendererUrl
  })

  test('creates main BrowserWindow and loads app protocol url in production mode', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    expect(win).toBeDefined()
    expect(win.loadURL).toHaveBeenCalledWith('app://index.html')
    expect(getMainWindow()).toBe(win)
  })

  test('attaches kiosk state sync on creation', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    expect(attachKioskStateSync).toHaveBeenCalledWith(runtimeState)
  })

  test('configures permission and usb handlers', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    expect(win.webContents.session.setPermissionCheckHandler).toHaveBeenCalled()
    expect(win.webContents.session.setPermissionRequestHandler).toHaveBeenCalled()
    expect(win.webContents.session.setUSBProtectedClassesHandler).toHaveBeenCalled()
    expect(session.defaultSession.webRequest.onHeadersReceived).toHaveBeenCalled()
  })

  test('ready-to-show applies size, shows window, sets zoom and attaches renderer', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 900,
        mainScreenHeight: 500,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 125
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const readyHandler = win.once.mock.calls.find(
      ([event]: any[]) => event === 'ready-to-show'
    )?.[1]

    expect(readyHandler).toBeDefined()
    readyHandler()

    expect(applyWindowedContentSize).toHaveBeenCalledWith(win, 900, 500)
    expect(win.show).toHaveBeenCalled()
    expect(win.webContents.setZoomFactor).toHaveBeenCalledWith(1.25)
    expect(pushSettingsToRenderer).toHaveBeenCalledWith(runtimeState, {
      kiosk: { main: false, dash: false, aux: false }
    })
    expect(services.projectionService.attachRenderer).toHaveBeenCalledWith(win.webContents)
  })

  test('ready-to-show opens devtools in dev mode', async () => {
    ;(isDev as Mock).mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const mainWin = browserWindowInstances[0]
    const readyHandler = mainWin.once.mock.calls.find(
      ([event]: any[]) => event === 'ready-to-show'
    )?.[1]
    readyHandler()

    expect(mainWin.webContents.openDevTools).toHaveBeenCalledWith({ mode: 'detach' })
    ;(isDev as Mock).mockReturnValue(false)
  })

  test('uses ELECTRON_RENDERER_URL in dev mode', async () => {
    ;(isDev as Mock).mockReturnValue(true)
    process.env.ELECTRON_RENDERER_URL = 'http://localhost:5173'

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const mainWin = browserWindowInstances[0]
    expect(mainWin.loadURL).toHaveBeenCalledWith('http://localhost:5173')
    ;(isDev as Mock).mockReturnValue(false)
  })

  test('creates extra dev windows in dev mode', async () => {
    ;(isDev as Mock).mockReturnValue(true)
    process.env.ELECTRON_RENDERER_URL = 'http://localhost:5173'

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    expect(browserWindowInstances).toHaveLength(3)
    expect(browserWindowInstances[1].loadURL).toHaveBeenCalledWith('chrome://gpu')
    expect(browserWindowInstances[2].loadURL).toHaveBeenCalledWith('chrome://media-internals')
    ;(isDev as Mock).mockReturnValue(false)
  })

  test('setWindowOpenHandler opens external urls and denies window creation', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.setWindowOpenHandler.mock.calls[0][0]

    const result = handler({ url: 'https://example.com' })

    expect(shell.openExternal).toHaveBeenCalledWith('https://example.com')
    expect(result).toEqual({ action: 'deny' })
  })

  test('mac fullscreen handlers sync aspect ratio and kiosk state', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 1000,
        mainScreenHeight: 600,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const enterHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'enter-full-screen'
    )?.[1]
    const leaveHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'leave-full-screen'
    )?.[1]

    enterHandler()
    expect(applyAspectRatioFullscreen).toHaveBeenCalledWith(win, 1000, 600)
    expect(persistKioskAndBroadcast).toHaveBeenCalledWith(true, runtimeState)

    leaveHandler()
    expect(applyAspectRatioWindowed).toHaveBeenCalledWith(win, 1000, 600)
    expect(persistKioskAndBroadcast).toHaveBeenCalledWith(false, runtimeState)
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })

  test('mac leave-full-screen handler clears suppressNextFsSync without syncing', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 1000,
        mainScreenHeight: 600,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false,
      suppressNextFsSync: true
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const leaveHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'leave-full-screen'
    )?.[1]

    leaveHandler()

    expect(runtimeState.suppressNextFsSync).toBe(false)
    expect(applyAspectRatioWindowed).not.toHaveBeenCalled()
    expect(persistKioskAndBroadcast).not.toHaveBeenCalled()
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })

  test('close hides mac window instead of quitting when not quitting', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const closeHandler = win.on.mock.calls.find(([event]: any[]) => event === 'close')?.[1]
    const preventDefault = vi.fn()

    closeHandler({ preventDefault })

    expect(preventDefault).toHaveBeenCalled()
    expect(win.hide).toHaveBeenCalled()
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })

  test('close exits fullscreen first on mac before hiding', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    win.isFullScreen.mockReturnValue(true)

    const closeHandler = win.on.mock.calls.find(([event]: any[]) => event === 'close')?.[1]
    const preventDefault = vi.fn()

    closeHandler({ preventDefault })

    expect(preventDefault).toHaveBeenCalled()
    expect(runtimeState.suppressNextFsSync).toBe(true)
    expect(win.once).toHaveBeenCalledWith('leave-full-screen', expect.any(Function))
    expect(win.setFullScreen).toHaveBeenCalledWith(false)
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })

  test('ready-to-show enters kiosk on linux when configured', async () => {
    const setImmediateSpy = vi.spyOn(global, 'setImmediate').mockImplementation(function (fn: any) {
      fn()
      return 0 as any
    } as any)
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const readyHandler = win.once.mock.calls.find(
      ([event]: any[]) => event === 'ready-to-show'
    )?.[1]
    readyHandler()

    expect(win.setKiosk).toHaveBeenCalledWith(true)
    expect(screen.getDisplayMatching).toHaveBeenCalled()
    expect(win.setContentSize).toHaveBeenCalledWith(1920, 1080)

    setImmediateSpy.mockRestore()
  })

  test('permission request handler allows supported permission', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.session.setPermissionRequestHandler.mock.calls[0][0]
    const cb = vi.fn()

    handler({}, 'usb', cb)

    expect(cb).toHaveBeenCalledWith(true)
  })

  test('permission request handler rejects unsupported permission', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.session.setPermissionRequestHandler.mock.calls[0][0]
    const cb = vi.fn()

    handler({}, 'notifications', cb)

    expect(cb).toHaveBeenCalledWith(false)
  })

  test('usb protected classes handler keeps only allowed classes', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.session.setUSBProtectedClassesHandler.mock.calls[0][0]

    const result = handler({
      protectedClasses: ['audio', 'hid', 'video', 'mass-storage', 'vendor-specific']
    })

    expect(result).toEqual(['audio', 'video', 'vendor-specific'])
  })

  test('headers received handler injects COOP COEP and CORP headers', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const handler = (session.defaultSession.webRequest.onHeadersReceived as Mock).mock.calls[0][1]
    const cb = vi.fn()

    handler(
      {
        responseHeaders: {
          Existing: ['x']
        }
      },
      cb
    )

    expect(cb).toHaveBeenCalledWith({
      responseHeaders: {
        Existing: ['x'],
        'Cross-Origin-Opener-Policy': ['same-origin'],
        'Cross-Origin-Embedder-Policy': ['require-corp'],
        'Cross-Origin-Resource-Policy': ['same-site']
      }
    })
  })

  test('leaves the custom proxy answers with the headers they carry', async () => {
    const { customProxy } = await import('@main/services/custom/CustomProxy')
    const url = vi.spyOn(customProxy, 'url').mockReturnValue('http://127.0.0.1:5555/')
    const runtimeState = {
      config: { kiosk: { main: false, dash: false, aux: false }, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const handler = (session.defaultSession.webRequest.onHeadersReceived as Mock).mock.calls[0][1]
    const cb = vi.fn()

    handler({ url: 'http://127.0.0.1:5555/js/app.js', responseHeaders: { Existing: ['x'] } }, cb)

    expect(cb).toHaveBeenCalledWith({})
    url.mockRestore()
  })

  test('savedBounds: ready-to-show re-applies position+size', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        mainScreenBounds: { x: 50, y: 60, width: 1024, height: 768 }
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any
    createMainWindow(runtimeState, services)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    expect(win.__opts.x).toBe(50)
    expect(win.__opts.width).toBe(1024)
    win.setBounds = vi.fn()
    const restoreCb = win.once.mock.calls.find(([e]: any[]) => e === 'ready-to-show')?.[1]
    restoreCb()
    expect(win.setBounds).toHaveBeenCalledWith({ x: 50, y: 60, width: 1024, height: 768 })
  })

  test('savedBounds: destroyed window skips the restore', async () => {
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        mainScreenBounds: { x: 50, y: 60, width: 1024, height: 768 }
      },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.isDestroyed = vi.fn(() => true)
    win.setBounds = vi.fn()
    const restoreCb = win.once.mock.calls.find(([e]: any[]) => e === 'ready-to-show')?.[1]
    restoreCb()
    expect(win.setBounds).not.toHaveBeenCalled()
  })

  test('invalid mainScreenBounds shape in config is ignored', async () => {
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480, mainScreenBounds: { x: 1 } },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    expect(win.__opts.width).toBe(800)
  })

  test('move event saves geometry after debounce', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.getPosition = vi.fn(() => [10, 20])
    win.getContentSize = vi.fn(() => [800, 480])
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).toHaveBeenCalledWith(
      runtimeState,
      expect.objectContaining({
        mainScreenBounds: { x: 10, y: 20, width: 800, height: 480 }
      })
    )
    vi.useRealTimers()
  })

  test('move event with unchanged bounds skips save', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        mainScreenBounds: { x: 10, y: 20, width: 800, height: 480 }
      },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.getPosition = vi.fn(() => [10, 20])
    win.getContentSize = vi.fn(() => [800, 480])
    saveSettings.mockClear()
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('move event skips save when window is in full-screen', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.isFullScreen = vi.fn(() => true)
    saveSettings.mockClear()
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('close calls app.quit when isQuitting=false on linux', async () => {
    const { app } = (await vi.importMock('electron')) as { app: { quit: Mock } }
    app.quit.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const closeCb = win.on.mock.calls.find(([e]: any[]) => e === 'close')?.[1]
    const evt = { preventDefault: vi.fn() }
    closeCb(evt)
    expect(evt.preventDefault).toHaveBeenCalled()
    expect(app.quit).toHaveBeenCalled()
  })

  test('close lets the window die when isQuitting=true', async () => {
    const { app } = (await vi.importMock('electron')) as { app: { quit: Mock } }
    app.quit.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: true
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const closeCb = win.on.mock.calls.find(([e]: any[]) => e === 'close')?.[1]
    const evt = { preventDefault: vi.fn() }
    closeCb(evt)
    expect(evt.preventDefault).not.toHaveBeenCalled()
    expect(app.quit).not.toHaveBeenCalled()
  })

  test('getMainWindow returns the most recently created window', async () => {
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    expect(getMainWindow()).toBe(browserWindowInstances[browserWindowInstances.length - 1])
  })

  test('ready-to-show enters fullscreen on mac when kiosk is configured', async () => {
    const setImmediateSpy = vi.spyOn(global, 'setImmediate').mockImplementation(function (fn: any) {
      fn()
      return 0 as any
    } as any)
    ;(isMacPlatform as Mock).mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const readyHandler = win.once.mock.calls.find(
      ([event]: any[]) => event === 'ready-to-show'
    )?.[1]

    readyHandler()

    expect(win.setFullScreen).toHaveBeenCalledWith(true)
    expect(win.setKiosk).not.toHaveBeenCalled()
    ;(isMacPlatform as Mock).mockReturnValue(false)
    setImmediateSpy.mockRestore()
  })

  test('compositor mode ignores saved bounds and enters fullscreen after the deferred timeout', async () => {
    vi.useFakeTimers()
    process.env.LIVI_COMPOSITOR = '1'
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        mainScreenBounds: { x: 5, y: 6, width: 700, height: 400 },
        kiosk: { main: true, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    expect(win.__opts.x).toBeUndefined()
    expect(win.__opts.transparent).toBe(true)
    const readyHandler = win.once.mock.calls.find(([e]: any[]) => e === 'ready-to-show')?.[1]
    readyHandler()
    expect(applyWindowedContentSize).not.toHaveBeenCalled()
    vi.advanceTimersByTime(400)
    expect(win.setContentSize).toHaveBeenCalledWith(1920, 1080)
    expect(win.setFullScreen).toHaveBeenCalledWith(true)
    delete process.env.LIVI_COMPOSITOR
    vi.useRealTimers()
  })

  test('compositor mode skips bounds persistence on move', async () => {
    vi.useFakeTimers()
    process.env.LIVI_COMPOSITOR = '1'
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.getPosition = vi.fn(() => [10, 20])
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    delete process.env.LIVI_COMPOSITOR
    vi.useRealTimers()
  })

  test('bounds persistence skips a destroyed window', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.isDestroyed = vi.fn(() => true)
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('bounds persistence skips a kiosk window', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.isKiosk = vi.fn(() => true)
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('bounds persistence skips when getPosition is unavailable', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('bounds persistence skips when getContentSize is unavailable', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.getPosition = vi.fn(() => [10, 20])
    win.getContentSize = undefined
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).not.toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('rapid move events debounce into a single save', async () => {
    vi.useFakeTimers()
    const { saveSettings } = (await vi.importMock('@main/ipc/utils')) as {
      saveSettings: Mock
    }
    saveSettings.mockClear()
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.getPosition = vi.fn(() => [10, 20])
    win.getContentSize = vi.fn(() => [800, 480])
    const moveCb = win.on.mock.calls.find(([e]: any[]) => e === 'move')?.[1]
    moveCb()
    vi.advanceTimersByTime(100)
    moveCb()
    vi.advanceTimersByTime(500)
    expect(saveSettings).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

  test('permission check handler allows only supported permissions', async () => {
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const handler = win.webContents.session.setPermissionCheckHandler.mock.calls[0][0]
    expect(handler({}, 'media')).toBe(true)
    expect(handler({}, 'geolocation')).toBe(false)
  })

  test('ready-to-show falls back to default size and zoom', async () => {
    const runtimeState = {
      config: { mainScreenWidth: 0, mainScreenHeight: 0 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: vi.fn() } } as any
    createMainWindow(runtimeState, services)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const readyHandler = win.once.mock.calls.find(([e]: any[]) => e === 'ready-to-show')?.[1]
    readyHandler()
    expect(applyWindowedContentSize).toHaveBeenCalledWith(win, 1200, 720)
    expect(win.webContents.setZoomFactor).toHaveBeenCalledWith(1)
  })

  test('goFullscreen aborts when the window was destroyed meanwhile', async () => {
    const setImmediateSpy = vi.spyOn(global, 'setImmediate').mockImplementation(function (fn: any) {
      fn()
      return 0 as any
    } as any)
    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        uiZoomPercent: 100
      },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.isDestroyed = vi.fn(() => true)
    const readyHandler = win.once.mock.calls.find(([e]: any[]) => e === 'ready-to-show')?.[1]
    readyHandler()
    expect(win.setKiosk).not.toHaveBeenCalled()
    expect(win.setFullScreen).not.toHaveBeenCalled()
    setImmediateSpy.mockRestore()
  })

  test('mac enter-full-screen respects suppressNextFsSync', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false,
      suppressNextFsSync: true
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const enterHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'enter-full-screen'
    )?.[1]
    enterHandler()
    expect(applyAspectRatioFullscreen).not.toHaveBeenCalled()
    expect(persistKioskAndBroadcast).not.toHaveBeenCalled()
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })

  test('mac fullscreen handlers fall back to default aspect dimensions', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)
    const runtimeState = {
      config: { mainScreenWidth: 0, mainScreenHeight: 0 },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const enterHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'enter-full-screen'
    )?.[1]
    const leaveHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'leave-full-screen'
    )?.[1]
    enterHandler()
    expect(applyAspectRatioFullscreen).toHaveBeenCalledWith(win, 800, 480)
    leaveHandler()
    expect(applyAspectRatioWindowed).toHaveBeenCalledWith(win, 800, 480)
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })

  test('render-process-gone logs the failure reason', async () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    const goneHandler = win.webContents.on.mock.calls.find(
      ([event]: any[]) => event === 'render-process-gone'
    )?.[1]
    goneHandler({}, { reason: 'crashed', exitCode: 1 })
    expect(errorSpy).toHaveBeenCalledWith('[window] renderer gone: reason=crashed exitCode=1')
    errorSpy.mockRestore()
  })

  test('close on mac hides the window once fullscreen is left', async () => {
    ;(isMacPlatform as Mock).mockReturnValue(true)
    const runtimeState = {
      config: { mainScreenWidth: 800, mainScreenHeight: 480 },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    createMainWindow(runtimeState, {
      projectionService: { attachRenderer: vi.fn() }
    } as any)
    const win = browserWindowInstances[browserWindowInstances.length - 1]
    win.isFullScreen.mockReturnValue(true)
    const closeHandler = win.on.mock.calls.find(([event]: any[]) => event === 'close')?.[1]
    closeHandler({ preventDefault: vi.fn() })
    const leaveOnce = win.once.mock.calls.find(([e]: any[]) => e === 'leave-full-screen')?.[1]
    leaveOnce()
    expect(win.hide).toHaveBeenCalled()
    ;(isMacPlatform as Mock).mockReturnValue(false)
  })
})
