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
;(global as any).window = Object.create(window)
;(global as any).window.api = {
  send: jest.fn(),
  receive: jest.fn()
}
