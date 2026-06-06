import './app/gpu'
import { bootstrapCompositor } from '@main/app/compositorBootstrap'
import { installMainProcessErrorHandlers } from '@main/app/errorHandler'
import { setupAppIdentity } from '@main/app/init'
import { setupLifecycle } from '@main/app/lifecycle'

installMainProcessErrorHandlers()

import { registerIpc } from '@main/ipc'
import { configEvents } from '@main/ipc/utils'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { checkAndInstallAaSudoers } from '@main/services/projection/driver/aa/aaSudoers'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetrySocket } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'
import { TelemetryStore } from '@main/services/telemetry/TelemetryStore'
import { runtimeStateProps } from '@main/types'
import type { Config } from '@shared/types'
import { app, BrowserWindow } from 'electron'
import { loadConfig } from './config/loadConfig'
import { USBService } from './services/usb/USBService'
import { checkAndInstallUdevRule } from './services/usb/udevRule'
import { setCompositorBackdrop, setMacBackdrop } from './services/video/GstVideo'
import { createMainWindow, getMainWindow } from './window/createWindow'
import { setupSecondaryWindows } from './window/secondaryWindows'

// Outer launcher hands off to the nested compositor and exits
if (bootstrapCompositor()) {
  app.exit(0)
}

app.whenReady().then(async () => {
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

  // Bottom plane = theme colour. Linux: the compositor draws the backdrop. macOS: paint the
  // window content view itself. Apply now and on every Mode change.
  const applyBackdrop = (darkMode: boolean): void => {
    setCompositorBackdrop(darkMode)
    for (const w of BrowserWindow.getAllWindows()) setMacBackdrop(w, darkMode)
  }
  applyBackdrop(runtimeState.config.darkMode)
  configEvents.on('changed', (next: Config) => applyBackdrop(next.darkMode))
  setupTelemetry({
    store: telemetryStore,
    projectionService,
    initialConfig: runtimeState.config
  })
  setupLifecycle(runtimeState, services)

  const win = getMainWindow()
  if (win) await checkAndInstallUdevRule(win)

  // Wireless AA needs root for BlueZ + hostapd + dnsmasq.
  if (win && runtimeState.config.wirelessAaEnabled === true && process.platform === 'linux') {
    await checkAndInstallAaSudoers(win)
  }

  projectionService.applyConfigPatch(runtimeState.config)

  await projectionService.autoStartIfNeeded()
})
