import { EventEmitter } from 'node:events'
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

vi.mock('@main/app/compositorBootstrap', () => ({
  bootstrapCompositor: vi.fn(() => false)
}))

vi.mock('@main/protocol/appProtocol', () => ({
  registerAppProtocol: vi.fn()
}))

vi.mock('@main/ipc', () => ({
  registerIpc: vi.fn()
}))

vi.mock('@main/ipc/utils', () => ({
  configEvents: new EventEmitter(),
  saveSettings: vi.fn()
}))

vi.mock('../ipc/app', () => ({
  restartApp: vi.fn(() => Promise.resolve())
}))

vi.mock('@main/app/init', () => ({
  setupAppIdentity: vi.fn()
}))

vi.mock('@main/services/projection/services/ProjectionService', () => ({
  ProjectionService: vi.fn().mockImplementation(function () {
    return {
      applyConfigPatch: vi.fn(),
      autoStartIfNeeded: vi.fn(async () => undefined),
      onProjectionEvent: vi.fn(() => () => undefined),
      dispatchRemoteInput: vi.fn()
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

vi.mock('@main/services/gvfsPhoneGuard', () => ({
  checkAndInstallGvfsGuard: vi.fn(() => Promise.resolve()),
  startPhoneSuppression: vi.fn()
}))

vi.mock('@main/services/packageCheck', () => ({
  checkMissingPackages: vi.fn(() => Promise.resolve({ dismissed: undefined }))
}))

vi.mock('@main/services/audio/SystemVolume', () => ({
  setSystemVolume: vi.fn(() => Promise.resolve()),
  startSystemVolumeMonitor: vi.fn(),
  stopSystemVolumeMonitor: vi.fn()
}))

vi.mock('../services/video/GstVideo', () => ({
  backdropHex: vi.fn(() => '#101010'),
  setCompositorBackdrop: vi.fn(),
  setMacBackdrop: vi.fn(),
  setStreamGamma: vi.fn()
}))

vi.mock('../window/secondaryWindows', () => ({
  setupSecondaryWindows: vi.fn()
}))

vi.mock('../services/carBridge/CarBridgeService', () => ({
  CarBridgeService: vi.fn().mockImplementation(function () {
    return { start: vi.fn(), stop: vi.fn(), handleEvent: vi.fn() }
  })
}))

async function mockReadyRunsCallback(): Promise<void> {
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
}

async function bootIndex(): Promise<void> {
  await import('@main/index')
  await new Promise((resolve) => setImmediate(resolve))
  await new Promise((resolve) => setImmediate(resolve))
}

describe('main index bootstrap', () => {
  const originalPlatform = process.platform

  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
    const { configEvents } = await import('@main/ipc/utils')
    configEvents.removeAllListeners()
  })

  afterEach(async () => {
    await new Promise((resolve) => setImmediate(resolve))
    Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  })

  test('bootstraps app on whenReady', async () => {
    const { app } = await import('electron')
    await mockReadyRunsCallback()

    const { loadConfig } = await import('../config/loadConfig')
    const { createMainWindow } = await import('../window/createWindow')
    const { setupSecondaryWindows } = await import('../window/secondaryWindows')
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

    await bootIndex()

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
    expect(setupSecondaryWindows).toHaveBeenCalledTimes(1)
    expect(setupTelemetry).toHaveBeenCalledTimes(1)
    expect(setupLifecycle).toHaveBeenCalledTimes(1)

    const service = (ProjectionService as Mock).mock.results[0].value
    expect(service.applyConfigPatch).toHaveBeenCalledTimes(1)
    expect(service.autoStartIfNeeded).toHaveBeenCalledTimes(1)

    // the bridge wiring: projection events flow in, HU keys flow back
    const { CarBridgeService } = await import('../services/carBridge/CarBridgeService')
    const bridge = (CarBridgeService as unknown as Mock).mock.results[0].value
    expect(bridge.start).toHaveBeenCalledTimes(1)
    const tap = service.onProjectionEvent.mock.calls[0][0]
    tap({ type: 'media-reset', reason: 'test' })
    expect(bridge.handleEvent).toHaveBeenCalledWith({ type: 'media-reset', reason: 'test' })
    bridge.onKey('next')
    expect(service.dispatchRemoteInput).toHaveBeenCalledWith('next')
    const { TelemetryStore } = await import('@main/services/telemetry/TelemetryStore')
    const mergeSpy = vi.spyOn(TelemetryStore.prototype, 'merge')
    bridge.onTelemetry({ speedKph: 73 })
    expect(mergeSpy).toHaveBeenCalledWith({ speedKph: 73 })
  })

  test('exits without booting when the outer launcher hands off to the compositor', async () => {
    const { app } = await import('electron')
    await mockReadyRunsCallback()
    const { bootstrapCompositor } = await import('@main/app/compositorBootstrap')
    ;(bootstrapCompositor as Mock).mockReturnValueOnce(true)
    const { ProjectionService } = await import(
      '@main/services/projection/services/ProjectionService'
    )

    await bootIndex()

    expect(app.exit as Mock).toHaveBeenCalledWith(0)
    expect(app.requestSingleInstanceLock as Mock).not.toHaveBeenCalled()
    expect(ProjectionService).not.toHaveBeenCalled()
  })

  test('exits and does not boot services when the single-instance lock is held', async () => {
    const { app } = await import('electron')
    ;(app.requestSingleInstanceLock as Mock).mockReturnValueOnce(false)

    const { ProjectionService } = await import(
      '@main/services/projection/services/ProjectionService'
    )
    const { TelemetrySocket } = await import('@main/services/Socket')

    await bootIndex()

    expect(app.exit as Mock).toHaveBeenCalledWith(0)
    expect(ProjectionService).not.toHaveBeenCalled()
    expect(TelemetrySocket).not.toHaveBeenCalled()
  })

  test('second-instance restores, shows and focuses the main window', async () => {
    const { app } = await import('electron')
    const { getMainWindow } = await import('../window/createWindow')

    await bootIndex()

    const call = (app.on as Mock).mock.calls.find((c) => c[0] === 'second-instance')
    expect(call).toBeDefined()
    const handler = call?.[1] as () => void

    ;(getMainWindow as Mock).mockReturnValueOnce(null)
    expect(() => handler()).not.toThrow()

    const restore = vi.fn()
    const show = vi.fn()
    const focus = vi.fn()
    ;(getMainWindow as Mock).mockReturnValueOnce({
      isMinimized: () => true,
      restore,
      show,
      focus
    })
    handler()
    expect(restore).toHaveBeenCalledTimes(1)
    expect(show).toHaveBeenCalledTimes(1)
    expect(focus).toHaveBeenCalledTimes(1)
    ;(getMainWindow as Mock).mockReturnValueOnce({
      isMinimized: () => false,
      restore,
      show,
      focus
    })
    handler()
    expect(restore).toHaveBeenCalledTimes(1)
    expect(show).toHaveBeenCalledTimes(2)
  })

  test('restarts instead of booting further when the udev rule was installed', async () => {
    await mockReadyRunsCallback()
    const { checkAndInstallUdevRule } = await import('../services/usb/udevRule')
    ;(checkAndInstallUdevRule as Mock).mockResolvedValueOnce(true)
    const { restartApp } = await import('../ipc/app')
    const { ProjectionService } = await import(
      '@main/services/projection/services/ProjectionService'
    )

    await bootIndex()

    expect(restartApp).toHaveBeenCalledTimes(1)
    const service = (ProjectionService as Mock).mock.results[0].value
    expect(service.autoStartIfNeeded).not.toHaveBeenCalled()
  })

  test('runs the BT sudoers installer when aa=true on linux', async () => {
    await mockReadyRunsCallback()

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
    const { checkAndInstallGvfsGuard, startPhoneSuppression } = await import(
      '@main/services/gvfsPhoneGuard'
    )

    await bootIndex()

    expect(checkAndInstallHelperSudoers).toHaveBeenCalled()
    expect(checkAndInstallGvfsGuard).toHaveBeenCalled()
    expect(startPhoneSuppression).toHaveBeenCalled()
  })

  test('skips the BT sudoers installer when aa=false and cp=false', async () => {
    await mockReadyRunsCallback()
    const { checkAndInstallHelperSudoers } = await import(
      '@main/services/projection/driver/helper/helperSudoers'
    )
    await bootIndex()
    expect(checkAndInstallHelperSudoers).not.toHaveBeenCalled()
  })

  test('persists newly dismissed packages on linux', async () => {
    await mockReadyRunsCallback()
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const { checkMissingPackages } = await import('@main/services/packageCheck')
    ;(checkMissingPackages as Mock).mockResolvedValueOnce({ dismissed: ['gstreamer'] })
    const { saveSettings } = await import('@main/ipc/utils')

    await bootIndex()

    expect(saveSettings).toHaveBeenCalledWith(expect.anything(), {
      dismissedPackages: ['gstreamer']
    })
  })

  test('skips the linux-only gvfs guard and package check off linux', async () => {
    await mockReadyRunsCallback()
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const { checkAndInstallGvfsGuard, startPhoneSuppression } = await import(
      '@main/services/gvfsPhoneGuard'
    )
    const { checkMissingPackages } = await import('@main/services/packageCheck')

    await bootIndex()

    expect(checkAndInstallGvfsGuard).not.toHaveBeenCalled()
    expect(startPhoneSuppression).not.toHaveBeenCalled()
    expect(checkMissingPackages).not.toHaveBeenCalled()
  })

  test('couples the head-unit volume to the system mixer when linked', async () => {
    await mockReadyRunsCallback()
    const cfg = {
      width: 800,
      height: 480,
      kiosk: false,
      huVolumeLinkSystem: true,
      huVolume: 0.5,
      audioOutputDevice: 'alsa:hw0'
    }
    const { loadConfig } = await import('../config/loadConfig')
    ;(loadConfig as Mock).mockReturnValueOnce(cfg)
    const { setSystemVolume, startSystemVolumeMonitor, stopSystemVolumeMonitor } = await import(
      '@main/services/audio/SystemVolume'
    )
    const { saveSettings } = await import('@main/ipc/utils')

    await bootIndex()

    expect(startSystemVolumeMonitor).toHaveBeenCalledTimes(1)
    expect(setSystemVolume).toHaveBeenCalledWith(0.5, 'alsa:hw0')

    const [getDevice, onLevel] = (startSystemVolumeMonitor as Mock).mock.calls[0] as [
      () => string | undefined,
      (level: number) => void
    ]
    expect(getDevice()).toBe('alsa:hw0')

    onLevel(0.8)
    expect(saveSettings).toHaveBeenCalledWith(expect.anything(), { huVolume: 0.8 })
    ;(saveSettings as Mock).mockClear()

    onLevel(0.5004)
    expect(saveSettings).not.toHaveBeenCalled()

    cfg.huVolumeLinkSystem = false
    onLevel(0.9)
    expect(saveSettings).not.toHaveBeenCalled()
    cfg.huVolumeLinkSystem = true

    const { configEvents } = await import('@main/ipc/utils')
    ;(setSystemVolume as Mock).mockClear()
    configEvents.emit('changed', { ...cfg, huVolume: 0.8004 })
    expect(setSystemVolume).not.toHaveBeenCalled()

    configEvents.emit('changed', { ...cfg, huVolume: 0.9 })
    expect(setSystemVolume).toHaveBeenCalledWith(0.9, 'alsa:hw0')

    configEvents.emit('changed', { ...cfg, huVolumeLinkSystem: false })
    expect(stopSystemVolumeMonitor).toHaveBeenCalled()
  })

  test('unlinked head-unit volume stops the system mixer monitor', async () => {
    await mockReadyRunsCallback()
    const { startSystemVolumeMonitor, stopSystemVolumeMonitor } = await import(
      '@main/services/audio/SystemVolume'
    )

    await bootIndex()

    expect(startSystemVolumeMonitor).not.toHaveBeenCalled()
    expect(stopSystemVolumeMonitor).toHaveBeenCalled()
  })

  test('applies backdrop and gamma now and on config changes', async () => {
    await mockReadyRunsCallback()
    const { BrowserWindow } = await import('electron')
    const fakeWin = {}
    ;(BrowserWindow.getAllWindows as Mock).mockReturnValue([fakeWin])
    const { backdropHex, setCompositorBackdrop, setMacBackdrop, setStreamGamma } = await import(
      '../services/video/GstVideo'
    )

    await bootIndex()

    expect(backdropHex).toHaveBeenCalledTimes(1)
    expect(setCompositorBackdrop).toHaveBeenCalledWith('#101010')
    expect(setMacBackdrop).toHaveBeenCalledWith(fakeWin, '#101010')
    expect(setStreamGamma).toHaveBeenCalledTimes(1)
    ;(BrowserWindow.getAllWindows as Mock).mockImplementation(() => [])

    const { configEvents } = await import('@main/ipc/utils')
    configEvents.emit('changed', { darkMode: true })
    expect(setCompositorBackdrop).toHaveBeenCalledTimes(2)
    expect(setStreamGamma).toHaveBeenCalledTimes(2)
  })
})
