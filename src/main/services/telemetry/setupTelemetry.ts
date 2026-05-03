import type { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { TelemetryEvents, TelemetrySocket } from '@main/services/Socket'
import { getMainWindow } from '@main/window/createWindow'

export function setupTelemetry(
  telemetrySocket: TelemetrySocket,
  projectionService?: ProjectionService
) {
  const mainWindow = getMainWindow()

  telemetrySocket.on(TelemetryEvents.Push, (payload) => {
    const msg = { ts: Date.now(), ...payload }
    if (mainWindow) {
      mainWindow.webContents.send(TelemetryEvents.Update, msg)

      if (typeof payload.reverse === 'boolean') {
        mainWindow.webContents.send(TelemetryEvents.Reverse, payload.reverse)
      }

      if (typeof payload.lights === 'boolean') {
        mainWindow.webContents.send(TelemetryEvents.Lights, payload.lights)
      }
    }

    try {
      projectionService?.publishVehicleData(payload)
    } catch (e) {
      console.warn('[setupTelemetry] publishVehicleData threw (ignored)', e)
    }
  })
}
