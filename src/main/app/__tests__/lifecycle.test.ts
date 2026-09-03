import { setupLifecycle } from '@main/app/lifecycle'
import { createMainWindow, getMainWindow } from '@main/window/createWindow'
import { app, BrowserWindow } from 'electron'
import type { Mock, MockInstance } from 'vitest'

vi.mock('@main/window/createWindow', () => ({
  createMainWindow: vi.fn(),
  getMainWindow: vi.fn(() => null)
}))

describe('setupLifecycle', () => {
  const originalPlatform = process.platform
  let killSpy: MockInstance

  beforeEach(async () => {
    vi.clearAllMocks()
    vi.useRealTimers()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    // before-quit ends with `process.kill(process.pid, 'SIGKILL')`; stub it
    // out so the test runner survives.
    killSpy = vi.spyOn(process, 'kill').mockImplementation(() => true)
  })

  afterEach(async () => {
    killSpy.mockRestore()
  })

  afterAll(async () => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  function getRegisteredHandlers(eventName: string): Array<(...args: unknown[]) => unknown> {
    return (app.on as Mock).mock.calls
      .filter(([name]) => name === eventName)
      .map(([, handler]) => handler as (...args: unknown[]) => unknown)
  }

  function getRegisteredHandler(eventName: string): ((...args: unknown[]) => unknown) | undefined {
    return getRegisteredHandlers(eventName)[0]
  }

  test('registers lifecycle listeners', async () => {
    setupLifecycle({ isQuitting: false } as never, {} as never)

    const registered = (app.on as Mock).mock.calls.map(([name]) => name)
    expect(registered).toEqual(
      expect.arrayContaining(['window-all-closed', 'activate', 'before-quit'])
    )
  })

  test('activate creates main window when no windows are open', async () => {
    ;(BrowserWindow.getAllWindows as Mock).mockReturnValue([])
    ;(getMainWindow as Mock).mockReturnValue(null)

    const runtimeState = { isQuitting: false } as never
    const services = { projectionService: {}, telemetrySocket: {} } as never

    setupLifecycle(runtimeState, services)

    const activate = getRegisteredHandler('activate')
    expect(activate).toBeDefined()

    activate?.()

    expect(createMainWindow).toHaveBeenCalledWith(runtimeState, services)
  })

  test('activate shows existing main window when a window already exists', async () => {
    const show = vi.fn()
    ;(BrowserWindow.getAllWindows as Mock).mockReturnValue([{}])
    ;(getMainWindow as Mock).mockReturnValue({ show })

    const runtimeState = { isQuitting: false } as never
    const services = { projectionService: {}, telemetrySocket: {} } as never

    setupLifecycle(runtimeState, services)

    const activate = getRegisteredHandler('activate')
    expect(activate).toBeDefined()

    activate?.()

    expect(createMainWindow).not.toHaveBeenCalled()
    expect(show).toHaveBeenCalledTimes(1)
  })

  test('window-all-closed quits app on non-darwin for both registered handlers', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    setupLifecycle({ isQuitting: false } as never, {} as never)

    const handlers = getRegisteredHandlers('window-all-closed')
    expect(handlers).toHaveLength(2)

    handlers[0]?.()
    handlers[1]?.()

    expect(app.quit).toHaveBeenCalledTimes(2)
  })

  test('window-all-closed does not quit app on darwin', async () => {
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
      beginShutdown: vi.fn(),
      disconnectPhone: vi.fn(() => Promise.resolve()),
      disconnectHostBtPhones: vi.fn(() => Promise.resolve()),
      shutdownWirelessSessions: vi.fn(() => Promise.resolve()),
      stopHelper: vi.fn(() => Promise.resolve()),
      stop: vi.fn(() => Promise.resolve())
    }
    const telemetrySocket = {
      disconnect: vi.fn(() => Promise.resolve())
    }

    const runtimeState = { isQuitting: true } as never
    setupLifecycle(runtimeState, { projectionService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: Mock }) => Promise<void>)
      | undefined

    const event = { preventDefault: vi.fn() }
    await beforeQuit?.(event)

    expect(event.preventDefault).not.toHaveBeenCalled()
    expect(projectionService.beginShutdown).not.toHaveBeenCalled()
    expect(app.quit).not.toHaveBeenCalled()
  })

  test.each([
    ['linux', 'linux'],
    ['darwin', 'darwin']
  ])('before-quit runs shutdown pipeline and quits app (%s watchdog window)', async (_l, plat) => {
    Object.defineProperty(process, 'platform', { value: plat, configurable: true })
    const projectionService = {
      beginShutdown: vi.fn(),
      disconnectPhone: vi.fn(() => Promise.resolve()),
      disconnectHostBtPhones: vi.fn(() => Promise.resolve()),
      shutdownWirelessSessions: vi.fn(() => Promise.resolve()),
      stopHelper: vi.fn(() => Promise.resolve()),
      stop: vi.fn(() => Promise.resolve())
    }
    const telemetrySocket = {
      disconnect: vi.fn(() => Promise.resolve())
    }

    const runtimeState = { isQuitting: false } as never
    setupLifecycle(runtimeState, { projectionService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: Mock }) => Promise<void>)
      | undefined

    expect(beforeQuit).toBeDefined()

    const event = { preventDefault: vi.fn() }
    await beforeQuit?.(event)
    await new Promise<void>((resolve) => setImmediate(resolve))

    expect(event.preventDefault).toHaveBeenCalledTimes(1)
    expect(runtimeState.isQuitting).toBe(true)

    expect(projectionService.beginShutdown).toHaveBeenCalledTimes(1)
    expect(projectionService.disconnectPhone).toHaveBeenCalledTimes(1)
    expect(telemetrySocket.disconnect).toHaveBeenCalledTimes(1)
    expect(projectionService.stop).toHaveBeenCalledTimes(1)
    expect(killSpy).toHaveBeenCalledWith(process.pid, 'SIGKILL')
  })

  test('before-quit logs warning when a shutdown step throws', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(function () {})

    const projectionService = {
      beginShutdown: vi.fn(function () {
        throw new Error('shutdown failed')
      }),
      disconnectPhone: vi.fn(() => Promise.resolve()),
      disconnectHostBtPhones: vi.fn(() => Promise.resolve()),
      shutdownWirelessSessions: vi.fn(() => Promise.resolve()),
      stopHelper: vi.fn(() => Promise.resolve()),
      stop: vi.fn(() => Promise.resolve())
    }
    const telemetrySocket = {
      disconnect: vi.fn(() => Promise.resolve())
    }

    const runtimeState = { isQuitting: false } as never
    setupLifecycle(runtimeState, { projectionService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: Mock }) => Promise<void>)
      | undefined

    const event = { preventDefault: vi.fn() }
    await beforeQuit?.(event)
    await new Promise<void>((resolve) => setImmediate(resolve))

    expect(warnSpy).toHaveBeenCalledWith('[MAIN] Error while quitting:', expect.any(Error))
    expect(killSpy).toHaveBeenCalledWith(process.pid, 'SIGKILL')
  })

  test('before-quit logs timeout warning when a step exceeds timeout', async () => {
    vi.useFakeTimers()

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(function () {})
    const logSpy = vi.spyOn(console, 'log').mockImplementation(function () {})

    const projectionService = {
      beginShutdown: vi.fn(),
      disconnectPhone: vi.fn(
        () =>
          new Promise<void>((resolve) => {
            setTimeout(resolve, 5000)
          })
      ),
      disconnectHostBtPhones: vi.fn(() => Promise.resolve()),
      shutdownWirelessSessions: vi.fn(() => Promise.resolve()),
      stopHelper: vi.fn(() => Promise.resolve()),
      stop: vi.fn(() => Promise.resolve())
    }

    const telemetrySocket = {
      disconnect: vi.fn(() => Promise.resolve())
    }

    const runtimeState = { isQuitting: false } as never
    setupLifecycle(runtimeState, { projectionService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: Mock }) => Promise<void>)
      | undefined

    expect(beforeQuit).toBeDefined()

    const promise = beforeQuit?.({ preventDefault: vi.fn() } as any)

    await vi.advanceTimersByTimeAsync(1000)
    await promise

    expect(warnSpy).toHaveBeenCalledWith(
      '[MAIN] before-quit timeout: projection.disconnectPhone() after 800ms'
    )
    expect(logSpy).toHaveBeenCalledWith(
      expect.stringContaining('[MAIN] before-quit step:start projection.disconnectPhone()')
    )
    expect(killSpy).toHaveBeenCalledWith(process.pid, 'SIGKILL')
  })

  test('before-quit logs watchdog warning when shutdown takes too long', async () => {
    vi.useFakeTimers()

    Object.defineProperty(process, 'platform', { value: 'linux' })

    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(function () {})
    vi.spyOn(console, 'log').mockImplementation(function () {})

    const projectionService = {
      beginShutdown: vi.fn(),
      disconnectPhone: vi.fn(() => Promise.resolve()),
      disconnectHostBtPhones: vi.fn(() => Promise.resolve()),
      shutdownWirelessSessions: vi.fn(() => Promise.resolve()),
      stopHelper: vi.fn(() => Promise.resolve()),
      stop: vi.fn(
        () =>
          new Promise<void>((resolve) => {
            setTimeout(resolve, 7000)
          })
      )
    }

    const telemetrySocket = {
      disconnect: vi.fn(() => Promise.resolve())
    }

    const runtimeState = { isQuitting: false } as never
    setupLifecycle(runtimeState, { projectionService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: Mock }) => Promise<void>)
      | undefined

    const promise = beforeQuit?.({ preventDefault: vi.fn() } as any)

    await vi.advanceTimersByTimeAsync(3100)

    expect(warnSpy).toHaveBeenCalledWith(
      '[MAIN] before-quit watchdog: giving up waiting after 3000ms'
    )

    await vi.advanceTimersByTimeAsync(10000)
    await promise

    expect(killSpy).toHaveBeenCalledWith(process.pid, 'SIGKILL')
  })

  test('before-quit uses a resolved fallback when telemetry disconnect is missing', async () => {
    vi.useFakeTimers()

    const logSpy = vi.spyOn(console, 'log').mockImplementation(function () {})

    const projectionService = {
      beginShutdown: vi.fn(),
      disconnectPhone: vi.fn(() => Promise.resolve()),
      disconnectHostBtPhones: vi.fn(() => Promise.resolve()),
      shutdownWirelessSessions: vi.fn(() => Promise.resolve()),
      stopHelper: vi.fn(() => Promise.resolve()),
      stop: vi.fn(() => Promise.resolve())
    }

    const telemetrySocket = {}

    const runtimeState = { isQuitting: false } as never
    setupLifecycle(runtimeState, { projectionService, telemetrySocket } as never)

    const beforeQuit = getRegisteredHandler('before-quit') as
      | ((e: { preventDefault: Mock }) => Promise<void>)
      | undefined

    const promise = beforeQuit?.({ preventDefault: vi.fn() } as any)

    await vi.advanceTimersByTimeAsync(1000)
    await promise

    expect(projectionService.beginShutdown).toHaveBeenCalledTimes(1)
    expect(projectionService.disconnectPhone).toHaveBeenCalledTimes(1)
    expect(projectionService.stop).toHaveBeenCalledTimes(1)

    expect(logSpy).toHaveBeenCalledWith(
      expect.stringContaining('[MAIN] before-quit step:start telemetrySocket.disconnect()')
    )
    expect(logSpy).toHaveBeenCalledWith(
      expect.stringContaining('[MAIN] before-quit step:start projection.disconnectPhone()')
    )

    expect(killSpy).toHaveBeenCalledWith(process.pid, 'SIGKILL')
  })
})
