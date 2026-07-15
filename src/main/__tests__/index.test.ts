import type { Mock } from 'vitest'

vi.mock('../config/loadConfig', () => ({
  loadConfig: vi.fn(function () {
    return { width: 800, height: 480, kiosk: false }
  })
}))

vi.mock('../window/createWindow', () => ({
  createMainWindow: vi.fn(),
  getMainWindow: vi.fn(function () {
    return {}
  })
}))

vi.mock('@main/app/lifecycle', () => ({
  setupLifecycle: vi.fn()
}))

vi.mock('@main/protocol/appProtocol', () => ({
  registerAppProtocol: vi.fn()
}))

vi.mock('@main/ipc', () => ({
  registerIpc: vi.fn()
}))

vi.mock('@main/app/init', () => ({
  setupAppIdentity: vi.fn()
}))

vi.mock('@main/services/projection/services/ProjectionService', () => ({
  ProjectionService: vi.fn().mockImplementation(function () {
    return {
      applyConfigPatch: vi.fn(),
      autoStartIfNeeded: vi.fn(async () => undefined)
    }
  })
}))

vi.mock('../services/usb/USBService', () => ({
  USBService: vi.fn().mockImplementation(function () {
    return {}
  })
}))

vi.mock('@main/services/Socket', () => ({
  TelemetrySocket: vi.fn().mockImplementation(function () {
    return { disconnect: vi.fn() }
  })
}))

vi.mock('@main/services/telemetry/setupTelemetry', () => ({
  setupTelemetry: vi.fn()
}))

vi.mock('../services/usb/udevRule', () => ({
  checkAndInstallUdevRule: vi.fn(() => Promise.resolve())
}))

vi.mock('@main/services/projection/driver/helper/helperSudoers', () => ({
  checkAndInstallHelperSudoers: vi.fn(() => Promise.resolve())
}))

describe('main index bootstrap', () => {
  const originalPlatform = process.platform

  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
  })

  afterEach(async () => {
    await new Promise((resolve) => setImmediate(resolve))
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  })

  test('bootstraps app on whenReady', async () => {
    const { app } = await import('electron')
    ;(app.whenReady as Mock).mockImplementation(
      () =>
        ({
          then: (cb: () => void) => {
            cb()
            return Promise.resolve()
          }
        }) as Promise<void>
    )

    const { loadConfig } = await import('../config/loadConfig')
    const { createMainWindow } = await import('../window/createWindow')
    const { setupLifecycle } = await import('@main/app/lifecycle')
    const { registerAppProtocol } = await import('@main/protocol/appProtocol')
    const { registerIpc } = await import('@main/ipc')
    const { setupAppIdentity } = await import('@main/app/init')
    const { setupTelemetry } = await import('@main/services/telemetry/setupTelemetry')
    const { ProjectionService } = await import(
      '@main/services/projection/services/ProjectionService'
    )
    const { USBService } = await import('../services/usb/USBService')
    const { TelemetrySocket } = await import('@main/services/Socket')

    await import('@main/index')
    await Promise.resolve()

    expect(app.whenReady as Mock).toHaveBeenCalledTimes(1)

    expect(ProjectionService).toHaveBeenCalledTimes(1)
    expect(USBService).toHaveBeenCalledTimes(1)
    expect(TelemetrySocket).toHaveBeenCalledTimes(1)
    expect((TelemetrySocket as Mock).mock.calls[0][1]).toBe(4000)

    expect(loadConfig).toHaveBeenCalledTimes(1)
    expect(setupAppIdentity).toHaveBeenCalledTimes(1)
    expect(registerAppProtocol).toHaveBeenCalledTimes(1)
    expect(registerIpc).toHaveBeenCalledTimes(1)
    expect(createMainWindow).toHaveBeenCalledTimes(1)
    expect(setupTelemetry).toHaveBeenCalledTimes(1)
    expect(setupLifecycle).toHaveBeenCalledTimes(1)
  })

  test('exits and does not boot services when the single-instance lock is held', async () => {
    const { app } = await import('electron')
    ;(app.requestSingleInstanceLock as Mock).mockReturnValueOnce(false)

    const { ProjectionService } = await import(
      '@main/services/projection/services/ProjectionService'
    )
    const { TelemetrySocket } = await import('@main/services/Socket')

    await import('@main/index')
    await Promise.resolve()

    expect(app.exit as Mock).toHaveBeenCalledWith(0)
    expect(ProjectionService).not.toHaveBeenCalled()
    expect(TelemetrySocket).not.toHaveBeenCalled()
  })

  test('runs the BT sudoers installer when aa=true on linux', async () => {
    const { app } = await import('electron')
    ;(app.whenReady as Mock).mockImplementation(
      () =>
        ({
          then: (cb: () => void) => {
            cb()
            return Promise.resolve()
          }
        }) as Promise<void>
    )

    const { loadConfig } = await import('../config/loadConfig')
    ;(loadConfig as Mock).mockReturnValueOnce({
      width: 800,
      height: 480,
      kiosk: false,
      wirelessAaEnabled: true
    })

    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })

    const { checkAndInstallHelperSudoers } = await import(
      '@main/services/projection/driver/helper/helperSudoers'
    )
    await import('@main/index')
    await Promise.resolve()
    await Promise.resolve()

    expect(checkAndInstallHelperSudoers).toHaveBeenCalled()
  })

  test('skips the BT sudoers installer when aa=false and cp=false', async () => {
    const { app } = await import('electron')
    ;(app.whenReady as Mock).mockImplementation(
      () =>
        ({
          then: (cb: () => void) => {
            cb()
            return Promise.resolve()
          }
        }) as Promise<void>
    )
    const { checkAndInstallHelperSudoers } = await import(
      '@main/services/projection/driver/helper/helperSudoers'
    )
    await import('@main/index')
    await Promise.resolve()
    await Promise.resolve()
    expect(checkAndInstallHelperSudoers).not.toHaveBeenCalled()
  })
})
