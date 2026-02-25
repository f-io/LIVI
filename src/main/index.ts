import { app } from 'electron'
import { loadConfig } from './config/loadConfig'
import { createMainWindow } from './window/createWindow'
import './app/gpu'
import { setupLifecycle } from '@main/app/lifecycle'
import { registerAppProtocol } from '@main/protocol/appProtocol'
import { registerIpc } from '@main/ipc'
import { runtimeStateProps } from '@main/types'
import { setupAppIdentity } from '@main/app/init'
import { CarplayService } from '@main/services/carplay/services/CarplayService'
import { USBService } from './services/usb/USBService'
import { TelemetrySocket } from '@main/services/Socket'
import { setupTelemetry } from '@main/services/telemetry/setupTelemetry'

app.whenReady().then(() => {
  const carplayService = new CarplayService()
  const usbService = new USBService(carplayService)
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
    carplayService,
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
