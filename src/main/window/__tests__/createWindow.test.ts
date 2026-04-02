import { is } from '@electron-toolkit/utils'
import { isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import { createMainWindow, getMainWindow } from '@main/window/createWindow'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  attachKioskStateSync,
  persistKioskAndBroadcast
} from '@main/window/utils'
import { screen, session, shell } from 'electron'

const browserWindowInstances: any[] = []

jest.mock('electron', () => {
  const BrowserWindow = jest.fn((opts) => {
    const instance = {
      __opts: opts,
      webContents: {
        session: {
          setPermissionCheckHandler: jest.fn(),
          setPermissionRequestHandler: jest.fn(),
          setUSBProtectedClassesHandler: jest.fn()
        },
        setWindowOpenHandler: jest.fn(),
        setZoomFactor: jest.fn(),
        openDevTools: jest.fn()
      },
      once: jest.fn(),
      on: jest.fn(),
      loadURL: jest.fn(),
      setKiosk: jest.fn(),
      setContentSize: jest.fn(),
      show: jest.fn(),
      hide: jest.fn(),
      getBounds: jest.fn(() => ({ x: 0, y: 0, width: 800, height: 480 })),
      isDestroyed: jest.fn(() => false),
      isFullScreen: jest.fn(() => false),
      setFullScreen: jest.fn()
    }
    browserWindowInstances.push(instance)
    return instance
  })

  return {
    BrowserWindow: Object.assign(BrowserWindow, {
      getAllWindows: jest.fn(() => [])
    }),
    session: {
      defaultSession: { webRequest: { onHeadersReceived: jest.fn() } }
    },
    shell: {
      openExternal: jest.fn()
    },
    screen: {
      getDisplayMatching: jest.fn(() => ({
        workAreaSize: { width: 1920, height: 1080 }
      }))
    }
  }
})

jest.mock('@electron-toolkit/utils', () => ({
  is: { dev: false }
}))

jest.mock('@main/utils', () => ({
  isMacPlatform: jest.fn(() => false),
  pushSettingsToRenderer: jest.fn()
}))

jest.mock('@main/window/utils', () => ({
  applyAspectRatioFullscreen: jest.fn(),
  applyAspectRatioWindowed: jest.fn(),
  applyWindowedContentSize: jest.fn(),
  attachKioskStateSync: jest.fn(),
  currentKiosk: jest.fn(() => false),
  persistKioskAndBroadcast: jest.fn()
}))

describe('createMainWindow', () => {
  const originalRendererUrl = process.env.ELECTRON_RENDERER_URL

  beforeEach(() => {
    browserWindowInstances.length = 0
    jest.clearAllMocks()
    process.env.ELECTRON_RENDERER_URL = originalRendererUrl
  })

  afterAll(() => {
    process.env.ELECTRON_RENDERER_URL = originalRendererUrl
  })

  test('creates main BrowserWindow and loads app protocol url in production mode', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    expect(win).toBeDefined()
    expect(win.loadURL).toHaveBeenCalledWith('app://index.html')
    expect(getMainWindow()).toBe(win)
  })

  test('attaches kiosk state sync on creation', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    expect(attachKioskStateSync).toHaveBeenCalledWith(runtimeState)
  })

  test('configures permission and usb handlers', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    expect(win.webContents.session.setPermissionCheckHandler).toHaveBeenCalled()
    expect(win.webContents.session.setPermissionRequestHandler).toHaveBeenCalled()
    expect(win.webContents.session.setUSBProtectedClassesHandler).toHaveBeenCalled()
    expect(session.defaultSession.webRequest.onHeadersReceived).toHaveBeenCalled()
  })

  test('ready-to-show applies size, shows window, sets zoom and attaches renderer', () => {
    const runtimeState = {
      config: { width: 900, height: 500, kiosk: false, uiZoomPercent: 125 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

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
    expect(pushSettingsToRenderer).toHaveBeenCalledWith(runtimeState, { kiosk: false })
    expect(services.projectionService.attachRenderer).toHaveBeenCalledWith(win.webContents)
  })

  test('ready-to-show opens devtools in dev mode', () => {
    ;(is as any).dev = true

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const mainWin = browserWindowInstances[0]
    const readyHandler = mainWin.once.mock.calls.find(
      ([event]: any[]) => event === 'ready-to-show'
    )?.[1]
    readyHandler()

    expect(mainWin.webContents.openDevTools).toHaveBeenCalledWith({ mode: 'detach' })
    ;(is as any).dev = false
  })

  test('uses ELECTRON_RENDERER_URL in dev mode', () => {
    ;(is as any).dev = true
    process.env.ELECTRON_RENDERER_URL = 'http://localhost:5173'

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const mainWin = browserWindowInstances[0]
    expect(mainWin.loadURL).toHaveBeenCalledWith('http://localhost:5173')
    ;(is as any).dev = false
  })

  test('creates extra dev windows in dev mode', () => {
    ;(is as any).dev = true
    process.env.ELECTRON_RENDERER_URL = 'http://localhost:5173'

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    expect(browserWindowInstances).toHaveLength(3)
    expect(browserWindowInstances[1].loadURL).toHaveBeenCalledWith('chrome://gpu')
    expect(browserWindowInstances[2].loadURL).toHaveBeenCalledWith('chrome://media-internals')
    ;(is as any).dev = false
  })

  test('setWindowOpenHandler opens external urls and denies window creation', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.setWindowOpenHandler.mock.calls[0][0]

    const result = handler({ url: 'https://example.com' })

    expect(shell.openExternal).toHaveBeenCalledWith('https://example.com')
    expect(result).toEqual({ action: 'deny' })
  })

  test('mac fullscreen handlers sync aspect ratio and kiosk state', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(true)

    const runtimeState = {
      config: { width: 1000, height: 600, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

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
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
  })

  test('mac leave-full-screen handler clears suppressNextFsSync without syncing', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(true)

    const runtimeState = {
      config: { width: 1000, height: 600, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false,
      suppressNextFsSync: true
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const leaveHandler = win.on.mock.calls.find(
      ([event]: any[]) => event === 'leave-full-screen'
    )?.[1]

    leaveHandler()

    expect(runtimeState.suppressNextFsSync).toBe(false)
    expect(applyAspectRatioWindowed).not.toHaveBeenCalled()
    expect(persistKioskAndBroadcast).not.toHaveBeenCalled()
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
  })

  test('close hides mac window instead of quitting when not quitting', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(true)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const closeHandler = win.on.mock.calls.find(([event]: any[]) => event === 'close')?.[1]
    const preventDefault = jest.fn()

    closeHandler({ preventDefault })

    expect(preventDefault).toHaveBeenCalled()
    expect(win.hide).toHaveBeenCalled()
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
  })

  test('close exits fullscreen first on mac before hiding', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(true)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false,
      suppressNextFsSync: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    win.isFullScreen.mockReturnValue(true)

    const closeHandler = win.on.mock.calls.find(([event]: any[]) => event === 'close')?.[1]
    const preventDefault = jest.fn()

    closeHandler({ preventDefault })

    expect(preventDefault).toHaveBeenCalled()
    expect(runtimeState.suppressNextFsSync).toBe(true)
    expect(win.once).toHaveBeenCalledWith('leave-full-screen', expect.any(Function))
    expect(win.setFullScreen).toHaveBeenCalledWith(false)
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
  })

  test('ready-to-show enters kiosk on linux when configured', () => {
    const setImmediateSpy = jest.spyOn(global, 'setImmediate').mockImplementation(((fn: any) => {
      fn()
      return 0 as any
    }) as any)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: true, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

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

  test('permission request handler allows supported permission', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.session.setPermissionRequestHandler.mock.calls[0][0]
    const cb = jest.fn()

    handler({}, 'usb', cb)

    expect(cb).toHaveBeenCalledWith(true)
  })

  test('permission request handler rejects unsupported permission', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.session.setPermissionRequestHandler.mock.calls[0][0]
    const cb = jest.fn()

    handler({}, 'notifications', cb)

    expect(cb).toHaveBeenCalledWith(false)
  })

  test('usb protected classes handler keeps only allowed classes', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const handler = win.webContents.session.setUSBProtectedClassesHandler.mock.calls[0][0]

    const result = handler({
      protectedClasses: ['audio', 'hid', 'video', 'mass-storage', 'vendor-specific']
    })

    expect(result).toEqual(['audio', 'video', 'vendor-specific'])
  })

  test('headers received handler injects COOP COEP and CORP headers', () => {
    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const handler = (session.defaultSession.webRequest.onHeadersReceived as jest.Mock).mock
      .calls[0][1]
    const cb = jest.fn()

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

  test('ready-to-show enters fullscreen on mac when kiosk is configured', () => {
    const setImmediateSpy = jest.spyOn(global, 'setImmediate').mockImplementation(((fn: any) => {
      fn()
      return 0 as any
    }) as any)

    ;(isMacPlatform as jest.Mock).mockReturnValue(true)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: true, uiZoomPercent: 100 },
      isQuitting: false
    } as any
    const services = { projectionService: { attachRenderer: jest.fn() } } as any

    createMainWindow(runtimeState, services)

    const win = browserWindowInstances[0]
    const readyHandler = win.once.mock.calls.find(
      ([event]: any[]) => event === 'ready-to-show'
    )?.[1]

    readyHandler()

    expect(win.setFullScreen).toHaveBeenCalledWith(true)
    expect(win.setKiosk).not.toHaveBeenCalled()
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
    setImmediateSpy.mockRestore()
  })
})
