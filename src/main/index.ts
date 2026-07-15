import './logTimestamps'
import './app/gpu'
import { bootstrapCompositor } from '@main/app/compositorBootstrap'
import { installMainProcessErrorHandlers } from '@main/app/errorHandler'
import { setupAppIdentity } from '@main/app/init'
import { setupLifecycle } from '@main/app/lifecycle'

installMainProcessErrorHandlers()

import { registerIpc } from '@main/ipc'
import { configEvents } from '@main/ipc/utils'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { checkAndInstallGvfsGuard, startPhoneSuppression } from '@main/services/gvfsPhoneGuard'
import { checkAndInstallHelperSudoers } from '@main/services/projection/driver/helper/helperSudoers'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetrySocket } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'
import { TelemetryStore } from '@main/services/telemetry/TelemetryStore'
import { runtimeStateProps } from '@main/types'
import type { Config } from '@shared/types'
import { app, BrowserWindow } from 'electron'
import { loadConfig } from './config/loadConfig'
import { restartApp } from './ipc/app'
import { USBService } from './services/usb/USBService'
import { checkAndInstallUdevRule } from './services/usb/udevRule'
import {
  backdropHex,
  setCompositorBackdrop,
  setMacBackdrop,
  setStreamGamma
} from './services/video/GstVideo'
import { createMainWindow, getMainWindow } from './window/createWindow'
import { setupSecondaryWindows } from './window/secondaryWindows'

// Outer launcher hands off to the nested compositor and exits
let bootAllowed = true
if (bootstrapCompositor()) {
  app.exit(0)
  bootAllowed = false
} else if (!app.requestSingleInstanceLock()) {
  // Another LIVI already owns the telemetry port and the USB device, do not start a rival
  app.exit(0)
  bootAllowed = false
} else {
  app.on('second-instance', () => {
    const win = getMainWindow()
    if (!win) return
    if (win.isMinimized()) win.restore()
    win.show()
    win.focus()
  })
}

app.whenReady().then(async () => {
  if (!bootAllowed) return
  const projectionService = new ProjectionService()
  const usbService = new USBService(projectionService)
  const telemetryStore = new TelemetryStore()
  const telemetrySocket = new TelemetrySocket(telemetryStore, 4000)
  const runtimeState: runtimeStateProps = {
    config: loadConfig(),
    telemetrySocket: null,
    isQuitting: false,
    suppressNextFsSync: false,
    wmExitedKiosk: false
  }

  runtimeState.telemetrySocket = telemetrySocket

  const services = {
    projectionService,
    usbService,
    telemetrySocket
  }

  setupAppIdentity()
  registerAppProtocol()
  registerIpc(runtimeState, services)
  createMainWindow(runtimeState, services)
  setupSecondaryWindows(runtimeState)

  // Bottom plane = theme background colour. Linux: the compositor draws the backdrop. macOS: paint
  // the window content view itself. Apply now and on every config change.
  const applyBackdrop = (cfg: Config): void => {
    const color = backdropHex(cfg.darkMode, cfg.backgroundColorDark, cfg.backgroundColorLight)
    setCompositorBackdrop(color)
    for (const w of BrowserWindow.getAllWindows()) setMacBackdrop(w, color)
  }
  applyBackdrop(runtimeState.config)
  configEvents.on('changed', (next: Config) => applyBackdrop(next))

  // video stream calibration, applied now and on every config change
  const applyGamma = (cfg: Config): void => {
    setStreamGamma(
      cfg.displayGamma,
      cfg.displayContrast,
      cfg.displayColorR,
      cfg.displayColorG,
      cfg.displayColorB
    )
  }
  applyGamma(runtimeState.config)
  configEvents.on('changed', (next: Config) => applyGamma(next))
  setupTelemetry({
    store: telemetryStore,
    projectionService,
    initialConfig: runtimeState.config
  })
  setupLifecycle(runtimeState, services)

  const win = getMainWindow()
  if (win && (await checkAndInstallUdevRule(win))) {
    await restartApp(runtimeState, services)
    return
  }

  if (
    win &&
    process.platform === 'linux' &&
    (runtimeState.config.wirelessAaEnabled === true ||
      runtimeState.config.wirelessCpEnabled === true)
  ) {
    await checkAndInstallHelperSudoers(win)
  }

  if (win && process.platform === 'linux') {
    await checkAndInstallGvfsGuard(win)
    startPhoneSuppression()
  }

  projectionService.applyConfigPatch(runtimeState.config)

  await projectionService.autoStartIfNeeded()
})
