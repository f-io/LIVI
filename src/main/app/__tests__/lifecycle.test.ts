import { app, BrowserWindow } from 'electron'
import { setupLifecycle } from '@main/app/lifecycle'
import { createMainWindow } from '@main/window/createWindow'

jest.mock('@main/window/createWindow', () => ({
  createMainWindow: jest.fn(),
  getMainWindow: jest.fn(() => null)
}))

describe('setupLifecycle', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  function getRegisteredHandler(eventName: string): ((...args: unknown[]) => unknown) | undefined {
    return (app.on as jest.Mock).mock.calls.find(([name]) => name === eventName)?.[1] as
      | ((...args: unknown[]) => unknown)
      | undefined
  }

  test('registers lifecycle listeners', () => {
    setupLifecycle({ isQuitting: false } as never, {} as never)

    const registered = (app.on as jest.Mock).mock.calls.map(([name]) => name)
    expect(registered).toEqual(
      expect.arrayContaining(['window-all-closed', 'activate', 'before-quit'])
    )
  })

  test('activate creates main window when no windows are open', () => {
    ;(BrowserWindow.getAllWindows as jest.Mock).mockReturnValue([])

    const runtimeState = { isQuitting: false } as never
    const services = { projectionService: {}, usbService: {}, telemetrySocket: {} } as never

    setupLifecycle(runtimeState, services)

    const activate = getRegisteredHandler('activate')
    expect(activate).toBeDefined()

    activate?.()

    expect(createMainWindow).toHaveBeenCalledWith(runtimeState, services)
  })

  test('before-quit runs shutdown pipeline and quits app', async () => {
    const projectionService = {
      beginShutdown: jest.fn(),
      disconnectPhone: jest.fn(() => Promise.resolve()),
      stop: jest.fn(() => Promise.resolve())
    }
    const usbService = {
      beginShutdown: jest.fn(),
      stop: jest.fn(() => Promise.resolve())
    }
    const telemetrySocket = {
      disconnect: jest.fn(() => Promise.resolve())
    }

    const runtimeState = { isQuitting: false } as never
    setupLifecycle(runtimeState, { projectionService, usbService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: jest.Mock }) => Promise<void>)
      | undefined

    expect(beforeQuit).toBeDefined()

    const event = { preventDefault: jest.fn() }
    await beforeQuit?.(event)
    await new Promise<void>((resolve) => setImmediate(resolve))

    expect(event.preventDefault).toHaveBeenCalledTimes(1)
    expect(runtimeState.isQuitting).toBe(true)

    expect(projectionService.beginShutdown).toHaveBeenCalledTimes(1)
    expect(usbService.beginShutdown).toHaveBeenCalledTimes(1)
    expect(usbService.stop).toHaveBeenCalledTimes(1)
    expect(projectionService.disconnectPhone).toHaveBeenCalledTimes(1)
    expect(telemetrySocket.disconnect).toHaveBeenCalledTimes(1)
    expect(projectionService.stop).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalledTimes(1)
  })
})
