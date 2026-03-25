import { app, shell } from 'electron'
import { spawn } from 'child_process'
import { registerAppIpc } from '@main/ipc/app'
import { restoreKioskAfterWmExit } from '@main/window/utils'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { getMainWindow } from '@main/window/createWindow'
import { isMacPlatform } from '@main/utils'

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: jest.fn(() => null)
}))

jest.mock('@main/utils', () => ({
  isMacPlatform: jest.fn(() => false)
}))

jest.mock('@main/window/utils', () => ({
  restoreKioskAfterWmExit: jest.fn()
}))

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: jest.fn(),
  registerIpcOn: jest.fn()
}))

jest.mock('child_process', () => ({
  spawn: jest.fn()
}))

const mockedGetMainWindow = getMainWindow as jest.Mock
const mockedIsMacPlatform = isMacPlatform as jest.Mock
const mockedRegisterIpcHandle = registerIpcHandle as jest.Mock
const mockedRegisterIpcOn = registerIpcOn as jest.Mock
const mockedSpawn = spawn as jest.Mock

describe('registerAppIpc', () => {
  const originalPlatform = process.platform
  const originalAppImage = process.env.APPIMAGE
  const originalAppDir = process.env.APPDIR
  const originalArgv0 = process.env.ARGV0
  const originalOwd = process.env.OWD

  beforeEach(() => {
    jest.clearAllMocks()
    jest.restoreAllMocks()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    mockedGetMainWindow.mockReturnValue(null)
    mockedIsMacPlatform.mockReturnValue(false)

    process.env.APPIMAGE = originalAppImage
    process.env.APPDIR = originalAppDir
    process.env.ARGV0 = originalArgv0
    process.env.OWD = originalOwd
  })

  afterAll(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    process.env.APPIMAGE = originalAppImage
    process.env.APPDIR = originalAppDir
    process.env.ARGV0 = originalArgv0
    process.env.OWD = originalOwd
  })

  function getHandle(channel: string) {
    return mockedRegisterIpcHandle.mock.calls.find(([name]) => name === channel)?.[1]
  }

  function getOn(channel: string) {
    return mockedRegisterIpcOn.mock.calls.find(([name]) => name === channel)?.[1]
  }

  test('registers app handlers and listener', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const registeredHandles = mockedRegisterIpcHandle.mock.calls.map((c) => c[0])
    const registeredOn = mockedRegisterIpcOn.mock.calls.map((c) => c[0])

    expect(registeredHandles).toEqual(
      expect.arrayContaining(['quit', 'app:quitApp', 'app:restartApp', 'app:openExternal'])
    )
    expect(registeredOn).toContain('app:user-activity')
  })

  test('quit handler calls app.quit on non-mac platforms', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const quitHandler = getHandle('quit') as (() => void) | undefined
    expect(quitHandler).toBeDefined()

    quitHandler?.()

    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('quit handler hides window on mac when not fullscreen', () => {
    const hide = jest.fn()
    mockedIsMacPlatform.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue({
      isFullScreen: jest.fn(() => false),
      hide
    })

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const quitHandler = getHandle('quit') as (() => void) | undefined
    quitHandler?.()

    expect(hide).toHaveBeenCalledTimes(1)
    expect(app.quit).not.toHaveBeenCalled()
  })

  test('quit handler exits fullscreen first on mac and suppresses next fs sync', () => {
    const once = jest.fn()
    const setFullScreen = jest.fn()
    mockedIsMacPlatform.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue({
      isFullScreen: jest.fn(() => true),
      once,
      setFullScreen,
      hide: jest.fn()
    })

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const quitHandler = getHandle('quit') as (() => void) | undefined
    quitHandler?.()

    expect(runtimeState.suppressNextFsSync).toBe(true)
    expect(once).toHaveBeenCalledWith('leave-full-screen', expect.any(Function))
    expect(setFullScreen).toHaveBeenCalledWith(false)
  })

  test('app:quitApp calls app.quit when app is not quitting', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const quitAppHandler = getHandle('app:quitApp') as (() => void) | undefined

    expect(quitAppHandler).toBeDefined()
    quitAppHandler?.()

    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:quitApp does nothing when already quitting', () => {
    const runtimeState = { isQuitting: true, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const quitAppHandler = getHandle('app:quitApp') as (() => void) | undefined
    quitAppHandler?.()

    expect(app.quit).not.toHaveBeenCalled()
  })

  test('app:user-activity triggers kiosk restore sync', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const userActivityListener = getOn('app:user-activity') as (() => void) | undefined

    expect(userActivityListener).toBeDefined()
    userActivityListener?.()

    expect(restoreKioskAfterWmExit).toHaveBeenCalledWith(runtimeState)
  })

  test('app:restartApp marks quitting, shuts down usb service, relaunches and exits', async () => {
    jest.spyOn(global, 'setTimeout').mockImplementation(((fn: TimerHandler) => {
      if (typeof fn === 'function') fn()
      return 0 as any
    }) as typeof setTimeout)

    const unref = jest.fn()
    mockedSpawn.mockReturnValue({ unref })
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'

    const beginShutdown = jest.fn()
    const gracefulReset = jest.fn().mockResolvedValue(undefined)

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { usbService: { beginShutdown, gracefulReset } } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(runtimeState.isQuitting).toBe(true)
    expect(beginShutdown).toHaveBeenCalledTimes(1)
    expect(gracefulReset).toHaveBeenCalledTimes(1)
    expect(unref).toHaveBeenCalledTimes(1)
    expect(app.exit).toHaveBeenCalledWith(0)
  })

  test('app:restartApp continues when gracefulReset fails', async () => {
    jest.spyOn(global, 'setTimeout').mockImplementation(((fn: TimerHandler) => {
      if (typeof fn === 'function') fn()
      return 0 as any
    }) as typeof setTimeout)
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})

    const unref = jest.fn()
    mockedSpawn.mockReturnValue({ unref })
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'

    const beginShutdown = jest.fn()
    const gracefulReset = jest.fn().mockRejectedValue(new Error('boom'))

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { usbService: { beginShutdown, gracefulReset } } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(beginShutdown).toHaveBeenCalledTimes(1)
    expect(gracefulReset).toHaveBeenCalledTimes(1)
    expect(unref).toHaveBeenCalledTimes(1)
    expect(warnSpy).toHaveBeenCalledWith(
      '[MAIN] gracefulReset failed (continuing restart):',
      expect.any(Error)
    )
    expect(app.exit).toHaveBeenCalledWith(0)
  })

  test('app:restartApp returns early when already quitting', async () => {
    const runtimeState = { isQuitting: true, suppressNextFsSync: false } as any
    const services = { usbService: { beginShutdown: jest.fn(), gracefulReset: jest.fn() } } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(app.relaunch).not.toHaveBeenCalled()
    expect(app.exit).not.toHaveBeenCalled()
  })

  test('app:restartApp uses APPIMAGE relaunch path on linux', async () => {
    jest.spyOn(global, 'setTimeout').mockImplementation(((fn: TimerHandler) => {
      if (typeof fn === 'function') fn()
      return 0 as any
    }) as typeof setTimeout)

    const unref = jest.fn()
    mockedSpawn.mockReturnValue({ unref })

    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'
    process.env.APPDIR = '/tmp/appdir'
    process.env.ARGV0 = 'argv0'
    process.env.OWD = '/tmp/owd'

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = {
      usbService: {
        beginShutdown: jest.fn(),
        gracefulReset: jest.fn().mockResolvedValue(undefined)
      }
    } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(mockedSpawn).toHaveBeenCalledWith(
      '/tmp/app.AppImage',
      [],
      expect.objectContaining({
        detached: true,
        stdio: 'ignore'
      })
    )

    const spawnOptions = mockedSpawn.mock.calls[0][2]
    expect(spawnOptions.env).not.toHaveProperty('APPIMAGE')
    expect(spawnOptions.env).not.toHaveProperty('APPDIR')
    expect(spawnOptions.env).not.toHaveProperty('ARGV0')
    expect(spawnOptions.env).not.toHaveProperty('OWD')

    expect(unref).toHaveBeenCalledTimes(1)
    expect(app.relaunch).not.toHaveBeenCalled()
    expect(app.exit).toHaveBeenCalledWith(0)
  })

  test('app:openExternal rejects empty urls', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const openExternalHandler = getHandle('app:openExternal') as
      | ((evt: unknown, url: string) => Promise<unknown>)
      | undefined

    await expect(openExternalHandler?.(undefined, '')).resolves.toEqual({
      ok: false,
      error: 'Empty URL'
    })
  })

  test('app:openExternal rejects non-http urls', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const openExternalHandler = getHandle('app:openExternal') as
      | ((evt: unknown, url: string) => Promise<unknown>)
      | undefined

    await expect(openExternalHandler?.(undefined, 'file:///tmp/test')).resolves.toEqual({
      ok: false,
      error: 'Only http/https URLs are allowed'
    })
  })

  test('app:openExternal opens valid http urls', async () => {
    ;(shell.openExternal as jest.Mock).mockResolvedValue(undefined)

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const openExternalHandler = getHandle('app:openExternal') as
      | ((evt: unknown, url: string) => Promise<unknown>)
      | undefined

    await expect(openExternalHandler?.(undefined, ' https://example.com ')).resolves.toEqual({
      ok: true
    })
    expect(shell.openExternal).toHaveBeenCalledWith('https://example.com')
  })
})
