import { getMainWindow } from '@main/window/createWindow'
import { TelemetryEvents, TelemetrySocket } from '@main/services/Socket'

export function setupTelemetry(telemetrySocket: TelemetrySocket) {
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
  })
}
