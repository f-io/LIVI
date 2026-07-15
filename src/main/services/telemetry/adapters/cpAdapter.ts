/**
 * Native CarPlay telemetry adapter.
 *
 * Feeds vehicle-side data to the native iAP2/CarPlay stack. nightMode (AirPlay
 * command) and GPS (iAP2 LocationInformation, NMEA) are wired; vehicle status
 * and EV (Apple Maps EV routing) follow in later passes.
 */

import { isWired, type TelemetryPayload } from '@shared/types/Telemetry'
import { encodeNmea } from '../nmea'
import type { TelemetryStore } from '../TelemetryStore'

/** The vehicle-data surface of the CarPlay subsystem (CpManager), fed by telemetry. */
export type CpTelemetrySink = {
  sendNightMode: (night: boolean) => void
  sendVehicleStatus: (status: { range?: number; outsideTemperature?: number }) => void
  sendLocation: (nmea: string) => void
}

export type CpAdapterDeps = {
  getCpDriver: () => CpTelemetrySink | null
  store: TelemetryStore
}

export type CpAdapterHandle = {
  off: () => void
  hydrate: () => void
}

export function attachCpAdapter({ store, getCpDriver }: CpAdapterDeps): CpAdapterHandle {
  let lastNightMode: boolean | undefined

  const onChange = (patch: TelemetryPayload, snap: TelemetryPayload): void => {
    const cp = getCpDriver()
    if (!cp) return

    if (
      'nightMode' in patch &&
      isWired('cp', 'nightMode') &&
      typeof patch.nightMode === 'boolean'
    ) {
      if (patch.nightMode !== lastNightMode) {
        lastNightMode = patch.nightMode
        cp.sendNightMode(patch.nightMode)
      }
    }

    // Vehicle status — range (km) + outside temperature (°C), pushed on change
    if (
      ('rangeKm' in patch && isWired('cp', 'rangeKm')) ||
      ('ambientC' in patch && isWired('cp', 'ambientC'))
    ) {
      const vs: { range?: number; outsideTemperature?: number } = {}
      if (isWired('cp', 'rangeKm') && typeof snap.rangeKm === 'number') {
        vs.range = Math.max(0, Math.min(65535, Math.round(snap.rangeKm)))
      }
      if (isWired('cp', 'ambientC') && typeof snap.ambientC === 'number') {
        vs.outsideTemperature = Math.round(snap.ambientC)
      }
      if ('range' in vs || 'outsideTemperature' in vs) cp.sendVehicleStatus(vs)
    }

    // GPS / GNSS — emit on every gps patch (producer rate-limits)
    if ('gps' in patch && isWired('cp', 'gps')) {
      const gps = snap.gps
      if (gps && typeof gps.lat === 'number' && typeof gps.lng === 'number') {
        const nmea = encodeNmea(
          gps.lat,
          gps.lng,
          gps.alt,
          gps.heading,
          gps.speedMs,
          gps.fixTs,
          gps.accuracyM
        )
        if (nmea) cp.sendLocation(nmea)
      }
    }
  }

  store.on('change', onChange)
  return {
    off: (): void => {
      store.off('change', onChange)
    },
    hydrate: (): void => {
      lastNightMode = undefined
      const snap = store.snapshot()
      if (Object.keys(snap).length === 0) return
      onChange(snap, snap)
    }
  }
}
