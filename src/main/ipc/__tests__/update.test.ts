import { registerIpcHandle } from '@main/ipc/register'
import { registerUpdateIpc } from '@main/ipc/update'
import { Updater } from '@main/ipc/update/updater'

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: jest.fn()
}))

jest.mock('@main/ipc/update/updater', () => ({
  Updater: jest.fn().mockImplementation(() => ({
    perform: jest.fn(),
    abort: jest.fn(),
    install: jest.fn()
  }))
}))

describe('registerUpdateIpc', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('creates Updater and registers update handlers', () => {
    const services = { projectionService: {}, usbService: {}, telemetrySocket: {} } as never

    registerUpdateIpc(services)

    expect(Updater).toHaveBeenCalledWith(services)

    const updaterInstance = (Updater as jest.Mock).mock.results[0].value

    expect(registerIpcHandle).toHaveBeenCalledWith('app:performUpdate', updaterInstance.perform)
    expect(registerIpcHandle).toHaveBeenCalledWith('app:abortUpdate', updaterInstance.abort)
    expect(registerIpcHandle).toHaveBeenCalledWith('app:beginInstall', updaterInstance.install)
  })
})
