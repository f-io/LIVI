import { app, BrowserWindow } from 'electron'
import { setupLifecycle } from '@main/app/lifecycle'
import { createMainWindow, getMainWindow } from '@main/window/createWindow'

jest.mock('@main/window/createWindow', () => ({
  createMainWindow: jest.fn(),
  getMainWindow: jest.fn(() => null)
}))

describe('setupLifecycle', () => {
  const originalPlatform = process.platform

  beforeEach(() => {
    jest.clearAllMocks()
    jest.useRealTimers()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  afterAll(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  function getRegisteredHandlers(eventName: string): Array<(...args: unknown[]) => unknown> {
    return (app.on as jest.Mock).mock.calls
      .filter(([name]) => name === eventName)
      .map(([, handler]) => handler as (...args: unknown[]) => unknown)
  }

  function getRegisteredHandler(eventName: string): ((...args: unknown[]) => unknown) | undefined {
    return getRegisteredHandlers(eventName)[0]
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
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    const runtimeState = { isQuitting: false } as never
    const services = { projectionService: {}, usbService: {}, telemetrySocket: {} } as never

    setupLifecycle(runtimeState, services)

    const activate = getRegisteredHandler('activate')
    expect(activate).toBeDefined()

    activate?.()

    expect(createMainWindow).toHaveBeenCalledWith(runtimeState, services)
  })

  test('activate shows existing main window when a window already exists', () => {
    const show = jest.fn()
    ;(BrowserWindow.getAllWindows as jest.Mock).mockReturnValue([{}])
    ;(getMainWindow as jest.Mock).mockReturnValue({ show })

    const runtimeState = { isQuitting: false } as never
    const services = { projectionService: {}, usbService: {}, telemetrySocket: {} } as never

    setupLifecycle(runtimeState, services)

    const activate = getRegisteredHandler('activate')
    expect(activate).toBeDefined()

    activate?.()

    expect(createMainWindow).not.toHaveBeenCalled()
    expect(show).toHaveBeenCalledTimes(1)
  })

  test('window-all-closed quits app on non-darwin for both registered handlers', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    setupLifecycle({ isQuitting: false } as never, {} as never)

    const handlers = getRegisteredHandlers('window-all-closed')
    expect(handlers).toHaveLength(2)

    handlers[0]?.()
    handlers[1]?.()

    expect(app.quit).toHaveBeenCalledTimes(2)
  })

  test('window-all-closed does not quit app on darwin', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    setupLifecycle({ isQuitting: false } as never, {} as never)

    const handlers = getRegisteredHandlers('window-all-closed')
    expect(handlers).toHaveLength(2)

    handlers[0]?.()
    handlers[1]?.()

    expect(app.quit).not.toHaveBeenCalled()
  })

  test('before-quit returns immediately when already quitting', async () => {
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

    const runtimeState = { isQuitting: true } as never
    setupLifecycle(runtimeState, { projectionService, usbService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: jest.Mock }) => Promise<void>)
      | undefined

    const event = { preventDefault: jest.fn() }
    await beforeQuit?.(event)

    expect(event.preventDefault).not.toHaveBeenCalled()
    expect(projectionService.beginShutdown).not.toHaveBeenCalled()
    expect(usbService.beginShutdown).not.toHaveBeenCalled()
    expect(app.quit).not.toHaveBeenCalled()
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

  test('before-quit logs warning when a shutdown step throws', async () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})

    const projectionService = {
      beginShutdown: jest.fn(() => {
        throw new Error('shutdown failed')
      }),
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

    const event = { preventDefault: jest.fn() }
    await beforeQuit?.(event)
    await new Promise<void>((resolve) => setImmediate(resolve))

    expect(warnSpy).toHaveBeenCalledWith('[MAIN] Error while quitting:', expect.any(Error))
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('before-quit logs timeout warning when a step exceeds timeout', async () => {
    jest.useFakeTimers()

    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const logSpy = jest.spyOn(console, 'log').mockImplementation(() => {})

    const projectionService = {
      beginShutdown: jest.fn(),
      disconnectPhone: jest.fn(
        () =>
          new Promise<void>((resolve) => {
            setTimeout(resolve, 5000)
          })
      ),
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

    const promise = beforeQuit?.({ preventDefault: jest.fn() } as any)

    await jest.advanceTimersByTimeAsync(1000)
    await promise

    expect(warnSpy).toHaveBeenCalledWith(
      '[MAIN] before-quit timeout: projection.disconnectPhone() after 800ms'
    )
    expect(logSpy).toHaveBeenCalledWith(
      expect.stringContaining('[MAIN] before-quit step:start projection.disconnectPhone()')
    )
    expect(app.quit).toHaveBeenCalledTimes(1)
  })
})
