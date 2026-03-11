export {}

jest.mock('electron', () => {
  const webContents = {
    send: jest.fn(),
    setZoomFactor: jest.fn(),
    session: {
      setPermissionCheckHandler: jest.fn(),
      setPermissionRequestHandler: jest.fn(),
      setUSBProtectedClassesHandler: jest.fn()
    },
    setWindowOpenHandler: jest.fn(),
    openDevTools: jest.fn()
  }

  return {
    app: {
      getPath: jest.fn(() => '/tmp'),
      getVersion: jest.fn(() => '0.0.0-test'),
      quit: jest.fn(),
      relaunch: jest.fn(),
      exit: jest.fn(),
      whenReady: jest.fn(() => Promise.resolve()),
      on: jest.fn(),
      once: jest.fn(),
      commandLine: { appendSwitch: jest.fn() }
    },
    ipcRenderer: {
      send: jest.fn(),
      on: jest.fn(),
      invoke: jest.fn(),
      removeListener: jest.fn()
    },
    ipcMain: {
      handle: jest.fn(),
      on: jest.fn(),
      removeHandler: jest.fn(),
      removeAllListeners: jest.fn()
    },
    BrowserWindow: Object.assign(
      jest.fn(() => ({ webContents })),
      {
        getAllWindows: jest.fn(() => [])
      }
    ),
    WebContents: jest.fn(),
    protocol: {
      registerSchemesAsPrivileged: jest.fn(),
      registerStreamProtocol: jest.fn()
    },
    session: {
      defaultSession: { webRequest: { onHeadersReceived: jest.fn() } }
    },
    shell: {
      openExternal: jest.fn()
    },
    screen: {
      getDisplayMatching: jest.fn(() => ({ workArea: { width: 1920, height: 1080, x: 0, y: 0 } })),
      getPrimaryDisplay: jest.fn(() => ({ workArea: { width: 1920, height: 1080, x: 0, y: 0 } }))
    },
    net: {
      request: jest.fn()
    }
  }
})

jest.mock('usb', () => ({
  usb: {
    on: jest.fn(),
    removeAllListeners: jest.fn(),
    unrefHotplugEvents: jest.fn()
  },
  getDeviceList: jest.fn(() => []),
  WebUSBDevice: jest.fn()
}))

declare global {
  interface Window {
    api: {
      send: jest.Mock
      receive: jest.Mock
    }
  }
}

if (typeof window !== 'undefined') {
  Object.defineProperty(window, 'api', {
    value: {
      send: jest.fn(),
      receive: jest.fn()
    },
    configurable: true
  })
}
