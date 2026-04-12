import './app/gpu'
import { setupAppIdentity } from '@main/app/init'
import { setupLifecycle } from '@main/app/lifecycle'
import { registerIpc } from '@main/ipc'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetrySocket } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'
import { runtimeStateProps } from '@main/types'
import { app } from 'electron'
import { loadConfig } from './config/loadConfig'
import { USBService } from './services/usb/USBService'
import { checkAndInstallUdevRule } from './services/usb/udevRule'
import { createMainWindow, getMainWindow } from './window/createWindow'

app.whenReady().then(async () => {
  const projectionService = new ProjectionService()
  const usbService = new USBService(projectionService)
  const telemetrySocket = new TelemetrySocket(4000)
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
  setupTelemetry(telemetrySocket)
  setupLifecycle(runtimeState, services)

  const win = getMainWindow()
  if (win) await checkAndInstallUdevRule(win)
})
