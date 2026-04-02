import { app } from 'electron'
import { loadConfig } from './config/loadConfig'
import { createMainWindow } from './window/createWindow'
import './app/gpu'
import { setupAppIdentity } from '@main/app/init'
import { setupLifecycle } from '@main/app/lifecycle'
import { registerIpc } from '@main/ipc'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetrySocket } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'
import { runtimeStateProps } from '@main/types'
import { USBService } from './services/usb/USBService'

app.whenReady().then(() => {
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
})
