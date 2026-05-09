import './app/gpu'
import { setupAppIdentity } from '@main/app/init'
import { setupLifecycle } from '@main/app/lifecycle'
import { registerIpc } from '@main/ipc'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { checkAndInstallAaSudoers } from '@main/services/projection/driver/aa/aaSudoers'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetrySocket } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'
import { TelemetryStore } from '@main/services/telemetry/TelemetryStore'
import { runtimeStateProps } from '@main/types'
import { app } from 'electron'
import { loadConfig } from './config/loadConfig'
import { USBService } from './services/usb/USBService'
import { checkAndInstallUdevRule } from './services/usb/udevRule'
import { createMainWindow, getMainWindow } from './window/createWindow'
import { setupSecondaryWindows } from './window/secondaryWindows'

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
  setupTelemetry({
    store: telemetryStore,
    projectionService,
    initialConfig: runtimeState.config
  })
  setupLifecycle(runtimeState, services)

  const win = getMainWindow()
  if (win) await checkAndInstallUdevRule(win)

  // Wireless AA needs root for BlueZ + hostapd + dnsmasq. We install the
  // sudoers drop-in once, on first run with `aa: true`.
  if (win && runtimeState.config.aa === true && process.platform === 'linux') {
    await checkAndInstallAaSudoers(win)
  }

  projectionService.applyConfigPatch(runtimeState.config)

  await projectionService.autoStartIfNeeded()
})
