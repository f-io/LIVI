import { app } from 'electron'
import { registerAppIpc } from '@main/ipc/app'
import { restoreKioskAfterWmExit } from '@main/window/utils'
import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'

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

describe('registerAppIpc', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('registers app handlers and listener', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const registeredHandles = (registerIpcHandle as jest.Mock).mock.calls.map((c) => c[0])
    const registeredOn = (registerIpcOn as jest.Mock).mock.calls.map((c) => c[0])

    expect(registeredHandles).toEqual(
      expect.arrayContaining(['quit', 'app:quitApp', 'app:restartApp'])
    )
    expect(registeredOn).toContain('app:user-activity')
  })

  test('app:quitApp calls app.quit when app is not quitting', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const quitAppHandler = (registerIpcHandle as jest.Mock).mock.calls.find(
      ([channel]) => channel === 'app:quitApp'
    )?.[1] as (() => void) | undefined

    expect(quitAppHandler).toBeDefined()
    quitAppHandler?.()

    expect(app.quit).toHaveBeenCalledTimes(1)
  })

  test('app:user-activity triggers kiosk restore sync', () => {
    const runtimeState = { isQuitting: false, suppressNextFsSync: false } as never
    const services = { usbService: {} } as never

    registerAppIpc(runtimeState, services)

    const userActivityListener = (registerIpcOn as jest.Mock).mock.calls.find(
      ([channel]) => channel === 'app:user-activity'
    )?.[1] as (() => void) | undefined

    expect(userActivityListener).toBeDefined()
    userActivityListener?.()

    expect(restoreKioskAfterWmExit).toHaveBeenCalledWith(runtimeState)
  })
})
