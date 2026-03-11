import { registerIpc } from '@main/ipc'
import { registerAppIpc } from '@main/ipc/app'
import { registerSettingsIpc } from '@main/ipc/settings'
import { registerUpdateIpc } from '@main/ipc/update'

jest.mock('@main/ipc/app', () => ({
  registerAppIpc: jest.fn()
}))

jest.mock('@main/ipc/settings', () => ({
  registerSettingsIpc: jest.fn()
}))

jest.mock('@main/ipc/update', () => ({
  registerUpdateIpc: jest.fn()
}))

describe('registerIpc', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('registers all IPC groups', () => {
    const runtimeState = { config: {} } as never
    const services = { projectionService: {}, usbService: {}, telemetrySocket: {} } as never

    registerIpc(runtimeState, services)

    expect(registerAppIpc).toHaveBeenCalledWith(runtimeState, services)
    expect(registerSettingsIpc).toHaveBeenCalledWith(runtimeState)
    expect(registerUpdateIpc).toHaveBeenCalledWith(services)
  })
})
