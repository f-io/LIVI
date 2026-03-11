import { ipcMain } from 'electron'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'

describe('register IPC helpers', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('registerIpcHandle replaces previous handler before registering a new one', () => {
    const handler = jest.fn()

    registerIpcHandle('test:handle', handler)

    expect(ipcMain.removeHandler).toHaveBeenCalledWith('test:handle')
    expect(ipcMain.handle).toHaveBeenCalledWith('test:handle', handler)
    expect((ipcMain.removeHandler as jest.Mock).mock.invocationCallOrder[0]).toBeLessThan(
      (ipcMain.handle as jest.Mock).mock.invocationCallOrder[0]
    )
  })

  test('registerIpcOn replaces previous listeners before registering a new one', () => {
    const listener = jest.fn()

    registerIpcOn('test:on', listener)

    expect(ipcMain.removeAllListeners).toHaveBeenCalledWith('test:on')
    expect(ipcMain.on).toHaveBeenCalledWith('test:on', listener)
    expect((ipcMain.removeAllListeners as jest.Mock).mock.invocationCallOrder[0]).toBeLessThan(
      (ipcMain.on as jest.Mock).mock.invocationCallOrder[0]
    )
  })
})
