export {}

jest.mock('electron', () => ({
  ipcRenderer: {
    send: jest.fn(),
    on: jest.fn(),
    invoke: jest.fn()
  },
  ipcMain: {
    handle: jest.fn(),
    on: jest.fn()
  },
  BrowserWindow: jest.fn()
}))

declare global {
  interface Window {
    api: {
      send: jest.Mock
      receive: jest.Mock
    }
  }
}

Object.defineProperty(window, 'api', {
  value: {
    send: jest.fn(),
    receive: jest.fn()
  },
  configurable: true
})
