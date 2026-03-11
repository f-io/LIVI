jest.mock('../config/loadConfig', () => ({
  loadConfig: jest.fn(() => ({ width: 800, height: 480, kiosk: false }))
}))

jest.mock('../window/createWindow', () => ({
  createMainWindow: jest.fn()
}))

jest.mock('@main/app/lifecycle', () => ({
  setupLifecycle: jest.fn()
}))

jest.mock('@main/protocol/appProtocol', () => ({
  registerAppProtocol: jest.fn()
}))

jest.mock('@main/ipc', () => ({
  registerIpc: jest.fn()
}))

jest.mock('@main/app/init', () => ({
  setupAppIdentity: jest.fn()
}))

jest.mock('@main/services/projection/services/ProjectionService', () => ({
  ProjectionService: jest.fn().mockImplementation(() => ({}))
}))

jest.mock('../services/usb/USBService', () => ({
  USBService: jest.fn().mockImplementation(() => ({}))
}))

jest.mock('@main/services/Socket', () => ({
  TelemetrySocket: jest.fn().mockImplementation(() => ({ disconnect: jest.fn() }))
}))

jest.mock('@main/services/telemetry/setupTelemetry', () => ({
  setupTelemetry: jest.fn()
}))

describe('main index bootstrap', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
  })

  test('bootstraps app on whenReady', async () => {
    const { app } = require('electron')
    ;(app.whenReady as jest.Mock).mockImplementation(
      () =>
        ({
          then: (cb: () => void) => {
            cb()
            return Promise.resolve()
          }
        }) as Promise<void>
    )

    const { loadConfig } = require('../config/loadConfig')
    const { createMainWindow } = require('../window/createWindow')
    const { setupLifecycle } = require('@main/app/lifecycle')
    const { registerAppProtocol } = require('@main/protocol/appProtocol')
    const { registerIpc } = require('@main/ipc')
    const { setupAppIdentity } = require('@main/app/init')
    const { setupTelemetry } = require('@main/services/telemetry/setupTelemetry')
    const { ProjectionService } = require('@main/services/projection/services/ProjectionService')
    const { USBService } = require('../services/usb/USBService')
    const { TelemetrySocket } = require('@main/services/Socket')

    require('@main/index')
    await Promise.resolve()

    expect(app.whenReady as jest.Mock).toHaveBeenCalledTimes(1)

    expect(ProjectionService).toHaveBeenCalledTimes(1)
    expect(USBService).toHaveBeenCalledTimes(1)
    expect(TelemetrySocket).toHaveBeenCalledWith(4000)

    expect(loadConfig).toHaveBeenCalledTimes(1)
    expect(setupAppIdentity).toHaveBeenCalledTimes(1)
    expect(registerAppProtocol).toHaveBeenCalledTimes(1)
    expect(registerIpc).toHaveBeenCalledTimes(1)
    expect(createMainWindow).toHaveBeenCalledTimes(1)
    expect(setupTelemetry).toHaveBeenCalledTimes(1)
    expect(setupLifecycle).toHaveBeenCalledTimes(1)
  })
})
