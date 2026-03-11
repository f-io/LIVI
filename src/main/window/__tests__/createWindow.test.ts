import { createMainWindow, getMainWindow } from '@main/window/createWindow'

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
      show: jest.fn(),
      hide: jest.fn(),
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
  fitWindowToWorkArea: jest.fn(),
  persistKioskAndBroadcast: jest.fn()
}))

describe('createMainWindow', () => {
  beforeEach(() => {
    browserWindowInstances.length = 0
    jest.clearAllMocks()
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
})
