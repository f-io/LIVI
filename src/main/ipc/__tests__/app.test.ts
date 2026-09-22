import { registerAppIpc } from '@main/ipc/app'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { hostPowerAvailable, requestPowerAction } from '@main/services/power/hostPower'
import { isMacPlatform } from '@main/utils'
import { broadcastToRenderers } from '@main/window/broadcast'
import { getMainWindow } from '@main/window/createWindow'
import { restoreKioskAfterWmExit } from '@main/window/utils'
import { spawn } from 'child_process'
import { app, shell } from 'electron'
import type { Mock } from 'vitest'

vi.mock('@main/window/createWindow', () => ({
  getMainWindow: vi.fn(() => null)
}))

vi.mock('@main/utils', () => ({
  isMacPlatform: vi.fn(() => false)
}))

vi.mock('@main/window/utils', () => ({
  restoreKioskAfterWmExit: vi.fn()
}))

vi.mock('@main/window/broadcast', () => ({
  broadcastToRenderers: vi.fn()
}))

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: vi.fn(),
  registerIpcOn: vi.fn()
}))

vi.mock('child_process', () => ({
  spawn: vi.fn()
}))

vi.mock('@main/services/video/GstVideo', () => ({
  compositorRestart: vi.fn(() => false)
}))

vi.mock('@main/protocol/appProtocol', () => ({
  CUSTOM_PAGE_URL: 'app://index.html/custom/index.html',
  CUSTOM_ICON_URL: 'app://index.html/custom/icon.svg',
  customPageExists: vi.fn(() => false),
  customIconExists: vi.fn(() => false)
}))

vi.mock('@main/services/custom/CustomProxy', () => ({
  customProxy: { start: vi.fn(async () => null) }
}))

vi.mock('@main/services/power/hostPower', () => ({
  hostPowerAvailable: vi.fn(() => false),
  requestPowerAction: vi.fn()
}))

const mockedGetMainWindow = getMainWindow as Mock
const mockedIsMacPlatform = isMacPlatform as Mock
const mockedRegisterIpcHandle = registerIpcHandle as Mock
const mockedRegisterIpcOn = registerIpcOn as Mock
const mockedSpawn = spawn as Mock
const mockedBroadcastToRenderers = broadcastToRenderers as Mock
const mockedHostPowerAvailable = hostPowerAvailable as Mock
const mockedRequestPowerAction = requestPowerAction as Mock

describe('registerAppIpc', () => {
  const originalPlatform = process.platform
  const originalAppImage = process.env.APPIMAGE
  const originalAppDir = process.env.APPDIR
  const originalArgv0 = process.env.ARGV0
  const originalOwd = process.env.OWD

  beforeEach(async () => {
    vi.clearAllMocks()
    vi.restoreAllMocks()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    mockedGetMainWindow.mockReturnValue(null)
    mockedIsMacPlatform.mockReturnValue(false)
    mockedHostPowerAvailable.mockReturnValue(false)

    process.env.APPIMAGE = originalAppImage
    process.env.APPDIR = originalAppDir
    process.env.ARGV0 = originalArgv0
    process.env.OWD = originalOwd
  })

  afterAll(async () => {
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

  // The restart path tears these two projection steps down before it relaunches.
  function projectionStub() {
    return {
      shutdownWirelessSessions: vi.fn().mockResolvedValue(undefined),
      stopHelper: vi.fn().mockResolvedValue(undefined)
    }
  }

  test('registers app handlers and listener', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const registeredHandles = mockedRegisterIpcHandle.mock.calls.map((c) => c[0])
    const registeredOn = mockedRegisterIpcOn.mock.calls.map((c) => c[0])

    expect(registeredHandles).toEqual(
      expect.arrayContaining(['quit', 'app:quitApp', 'app:restartApp', 'app:openExternal'])
    )
    expect(registeredOn).toEqual(expect.arrayContaining(['app:user-activity', 'app:media-key']))
  })

  test('quit handler calls app.quit on non-mac platforms', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const quitHandler = getHandle('quit') as (() => void) | undefined
    expect(quitHandler).toBeDefined()

    quitHandler?.()

    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('quit handler hides window on mac when not fullscreen', async () => {
    const hide = vi.fn()
    mockedIsMacPlatform.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue({
      isFullScreen: vi.fn(() => false),
      hide
    })

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const quitHandler = getHandle('quit') as (() => void) | undefined
    quitHandler?.()

    expect(hide).toHaveBeenCalledTimes(1)
    expect(app.quit).not.toHaveBeenCalled()
  })

  test('quit handler exits fullscreen first on mac and suppresses next fs sync', async () => {
    const once = vi.fn()
    const setFullScreen = vi.fn()
    mockedIsMacPlatform.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue({
      isFullScreen: vi.fn(() => true),
      once,
      setFullScreen,
      hide: vi.fn()
    })

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const quitHandler = getHandle('quit') as (() => void) | undefined
    quitHandler?.()

    expect(runtimeState.suppressNextFsSync).toBe(true)
    expect(once).toHaveBeenCalledWith('leave-full-screen', expect.any(Function))
    expect(setFullScreen).toHaveBeenCalledWith(false)

    const hide = (mockedGetMainWindow.mock.results[0].value as { hide: Mock }).hide
    const leaveHandler = once.mock.calls[0][1] as () => void
    leaveHandler()
    expect(hide).toHaveBeenCalledTimes(1)
  })

  test('app:customPageUrl names the proxy when one runs', async () => {
    const { customProxy } = await import('@main/services/custom/CustomProxy')
    ;(customProxy.start as Mock).mockResolvedValue('http://127.0.0.1:5555/')

    registerAppIpc({ isQuitting: false, config: { customUrl: '' } } as never, {} as never)
    const handler = getHandle('app:customPageUrl') as () => Promise<string | null>

    await expect(handler()).resolves.toBe('http://127.0.0.1:5555/')
  })

  test('app:customPageUrl names the local page when the folder holds one', async () => {
    const { customProxy } = await import('@main/services/custom/CustomProxy')
    const { customPageExists } = await import('@main/protocol/appProtocol')
    ;(customProxy.start as Mock).mockResolvedValue(null)
    ;(customPageExists as Mock).mockReturnValue(true)

    registerAppIpc({ isQuitting: false, config: { customUrl: '' } } as never, {} as never)
    const handler = getHandle('app:customPageUrl') as () => Promise<string | null>

    await expect(handler()).resolves.toBe('app://index.html/custom/index.html')
  })

  test('app:customPageUrl names nothing when neither exists', async () => {
    const { customProxy } = await import('@main/services/custom/CustomProxy')
    const { customPageExists } = await import('@main/protocol/appProtocol')
    ;(customProxy.start as Mock).mockResolvedValue(null)
    ;(customPageExists as Mock).mockReturnValue(false)

    registerAppIpc({ isQuitting: false, config: { customUrl: '' } } as never, {} as never)
    const handler = getHandle('app:customPageUrl') as () => Promise<string | null>

    await expect(handler()).resolves.toBeNull()
  })

  test('app:customIconUrl names the icon only when the folder has one', async () => {
    const { customIconExists } = await import('@main/protocol/appProtocol')
    registerAppIpc({ isQuitting: false, config: { customUrl: '' } } as never, {} as never)
    const handler = getHandle('app:customIconUrl') as () => string | null

    ;(customIconExists as Mock).mockReturnValue(true)
    expect(handler()).toBe('app://index.html/custom/icon.svg')

    ;(customIconExists as Mock).mockReturnValue(false)
    expect(handler()).toBeNull()
  })

  test('app:quitApp calls app.quit when app is not quitting', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const quitAppHandler = getHandle('app:quitApp') as (() => void) | undefined

    expect(quitAppHandler).toBeDefined()
    quitAppHandler?.()

    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:quitApp does nothing when already quitting', async () => {
    const runtimeState = { isQuitting: true, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const quitAppHandler = getHandle('app:quitApp') as (() => void) | undefined
    quitAppHandler?.()

    expect(app.quit).not.toHaveBeenCalled()
  })

  test('app:media-key fans the command out to all renderers', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const mediaKeyListener = getOn('app:media-key') as
      | ((evt: unknown, cmd: string) => void)
      | undefined
    expect(mediaKeyListener).toBeDefined()

    mediaKeyListener?.(undefined, 'playPause')
    expect(mockedBroadcastToRenderers).toHaveBeenCalledWith('app:media-key', 'playPause')
  })

  test('app:media-key ignores empty or non-string commands', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const mediaKeyListener = getOn('app:media-key') as
      | ((evt: unknown, cmd: unknown) => void)
      | undefined

    mediaKeyListener?.(undefined, '')
    mediaKeyListener?.(undefined, undefined)
    mediaKeyListener?.(undefined, 42)

    expect(mockedBroadcastToRenderers).not.toHaveBeenCalled()
  })

  test('app:user-activity triggers kiosk restore sync', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const userActivityListener = getOn('app:user-activity') as (() => void) | undefined

    expect(userActivityListener).toBeDefined()
    userActivityListener?.()

    expect(restoreKioskAfterWmExit).toHaveBeenCalledWith(runtimeState)
  })

  test('ui:path forwards the router path to the projection service', () => {
    const runtimeState = {} as never
    const setUiPath = vi.fn()
    const services = { projectionService: { setUiPath } } as never

    registerAppIpc(runtimeState, services)
    const listener = getOn('ui:path') as ((evt: unknown, path: string) => void) | undefined

    expect(listener).toBeDefined()
    listener?.({}, '/settings/general/display')
    expect(setUiPath).toHaveBeenCalledWith('/settings/general/display')

    listener?.({}, undefined)
    expect(setUiPath).toHaveBeenCalledWith('')
  })

  test('app:restartApp tears down the projection helper, relaunches and quits', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    const unref = vi.fn()
    mockedSpawn.mockReturnValue({ unref })
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'

    const projectionService = projectionStub()
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { projectionService } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(projectionService.shutdownWirelessSessions).toHaveBeenCalledTimes(1)
    expect(projectionService.stopHelper).toHaveBeenCalledTimes(1)
    expect(unref).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('a failing wireless teardown does not stop the restart', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as never
    } as typeof setTimeout)
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    mockedSpawn.mockReturnValue({ unref: vi.fn() })
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'
    const projectionService = projectionStub()
    projectionService.shutdownWirelessSessions = vi
      .fn()
      .mockRejectedValue(new Error('teardown boom'))
    registerAppIpc(
      { isQuitting: false, suppressNextFsSync: false } as never,
      { projectionService } as never
    )
    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()
    expect(warn).toHaveBeenCalledWith(
      expect.stringContaining('shutdownWirelessSessions failed'),
      expect.any(Error)
    )
    expect(app.quit).toHaveBeenCalledTimes(1)
    warn.mockRestore()
  })

  test('on the appliance app:quitApp powers the host off instead of just quitting', async () => {
    mockedHostPowerAvailable.mockReturnValue(true)
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    registerAppIpc(runtimeState, {} as never)

    const quitAppHandler = getHandle('app:quitApp') as (() => void) | undefined
    quitAppHandler?.()

    expect(mockedRequestPowerAction).toHaveBeenCalledWith('poweroff')
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('on a desktop app:quitApp asks for no power action', async () => {
    mockedHostPowerAvailable.mockReturnValue(false)
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    registerAppIpc(runtimeState, {} as never)

    const quitAppHandler = getHandle('app:quitApp') as (() => void) | undefined
    quitAppHandler?.()

    expect(mockedRequestPowerAction).not.toHaveBeenCalled()
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('on the appliance app:restartApp reboots the host and skips the app teardown', async () => {
    mockedHostPowerAvailable.mockReturnValue(true)
    const projectionService = projectionStub()
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { projectionService } as never

    registerAppIpc(runtimeState, services)
    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(mockedRequestPowerAction).toHaveBeenCalledWith('reboot')
    expect(app.quit).toHaveBeenCalledTimes(1)
    // before-quit owns the teardown, so the restart path must not start its own
    expect(projectionService.shutdownWirelessSessions).not.toHaveBeenCalled()
    expect(projectionService.stopHelper).not.toHaveBeenCalled()
  })

  test('app:restartApp ignores re-entrant calls while a restart is already in flight', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    delete process.env.APPIMAGE

    const projectionService = projectionStub()
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { projectionService } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined

    await Promise.all([restartHandler?.(), restartHandler?.(), restartHandler?.()])

    expect(projectionService.shutdownWirelessSessions).toHaveBeenCalledTimes(1)
    expect(projectionService.stopHelper).toHaveBeenCalledTimes(1)
    expect(app.relaunch).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:restartApp continues when stopHelper fails', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

    const unref = vi.fn()
    mockedSpawn.mockReturnValue({ unref })
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'

    const projectionService = projectionStub()
    projectionService.stopHelper.mockRejectedValue(new Error('boom'))

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { projectionService } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(projectionService.stopHelper).toHaveBeenCalledTimes(1)
    expect(unref).toHaveBeenCalledTimes(1)
    expect(warnSpy).toHaveBeenCalledWith(
      '[MAIN] stopHelper failed (continuing restart):',
      expect.any(Error)
    )
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:restartApp returns early when already quitting', async () => {
    const runtimeState = { isQuitting: true, suppressNextFsSync: false } as any
    const services = { projectionService: projectionStub() } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(app.relaunch).not.toHaveBeenCalled()
    expect(app.quit).not.toHaveBeenCalled()
  })

  test('app:restartApp uses APPIMAGE relaunch path on linux', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    const unref = vi.fn()
    mockedSpawn.mockReturnValue({ unref })

    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.APPIMAGE = '/tmp/app.AppImage'
    process.env.APPDIR = '/tmp/appdir'
    process.env.ARGV0 = 'argv0'
    process.env.OWD = '/tmp/owd'

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { projectionService: projectionStub() } as any

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
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:openExternal rejects empty urls', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

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
    const services = {} as never

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
    ;(shell.openExternal as Mock).mockResolvedValue(undefined)

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const openExternalHandler = getHandle('app:openExternal') as
      | ((evt: unknown, url: string) => Promise<unknown>)
      | undefined

    await expect(openExternalHandler?.(undefined, ' https://example.com ')).resolves.toEqual({
      ok: true
    })
    expect(shell.openExternal).toHaveBeenCalledWith('https://example.com')
  })

  test('app:restartApp relaunches and quits on non-APPIMAGE path', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    delete process.env.APPIMAGE

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { projectionService: projectionStub() } as any

    registerAppIpc(runtimeState, services)

    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(mockedSpawn).not.toHaveBeenCalled()
    expect(app.relaunch).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:restartApp lets the compositor re-exec and quits itself', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    const { compositorRestart } = await import('@main/services/video/GstVideo')
    ;(compositorRestart as Mock).mockReturnValueOnce(true)

    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as any
    const services = { projectionService: projectionStub() } as any

    registerAppIpc(runtimeState, services)
    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(runtimeState.isQuitting).toBe(true)
    expect(app.quit).toHaveBeenCalledTimes(1)
    expect(app.relaunch).not.toHaveBeenCalled()
    expect(mockedSpawn).not.toHaveBeenCalled()
  })

  test('app:restartApp awaits wireless teardown and telemetry disconnect', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    delete process.env.APPIMAGE

    const projectionService = projectionStub()
    const disconnect = vi.fn().mockResolvedValue(undefined)

    const runtimeState = {
      isQuitting: false,
      suppressNextFsSync: false,
      telemetrySocket: { disconnect }
    } as any
    const services = { projectionService } as any

    registerAppIpc(runtimeState, services)
    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(projectionService.shutdownWirelessSessions).toHaveBeenCalledTimes(1)
    expect(disconnect).toHaveBeenCalledTimes(1)
    expect(app.relaunch).toHaveBeenCalledTimes(1)
    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:restartApp continues when the telemetry disconnect rejects', async () => {
    vi.spyOn(global, 'setTimeout').mockImplementation(function (fn: TimerHandler) {
      if (typeof fn === 'function') fn()
      return 0 as any
    } as typeof setTimeout)
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    delete process.env.APPIMAGE

    const runtimeState = {
      isQuitting: false,
      suppressNextFsSync: false,
      telemetrySocket: { disconnect: vi.fn().mockRejectedValue(new Error('gone')) }
    } as any
    const services = { projectionService: projectionStub() } as any

    registerAppIpc(runtimeState, services)
    const restartHandler = getHandle('app:restartApp') as (() => Promise<void>) | undefined
    await restartHandler?.()

    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:openExternal rejects undefined urls via nullish fallback', async () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = {} as never

    registerAppIpc(runtimeState, services)

    const openExternalHandler = getHandle('app:openExternal') as
      | ((evt: unknown, url?: string) => Promise<unknown>)
      | undefined

    await expect(openExternalHandler?.(undefined, undefined)).resolves.toEqual({
      ok: false,
      error: 'Empty URL'
    })
  })
})
