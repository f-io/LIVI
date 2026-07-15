/**
 * Carlinkit dongle (CarPlay/AA bridge) telemetry adapter.
 *
 * The dongle is a fixed-firmware bridge box, not a vehicle bus — only two
 * vehicle-side data points are ever shipped to it:
 *
 *   • GPS / GNSS         → SendGnssData(nmeaText)
 *   • nightMode boolean  → SendBoolean(NIGHT_MODE)
 *
 * Everything else from `TelemetryPayload` is HU-side and routed elsewhere.
 *
 * GPS encoding: minimal NMEA-0183 stream of `$GPGGA` + `$GPRMC` per push
 */

import type { DongleDriver } from '@projection/driver/dongle/dongleDriver'
import { FileAddress, SendBoolean } from '@projection/messages/sendable'
import { isWired, type TelemetryPayload } from '@shared/types/Telemetry'
import { encodeNmea } from '../nmea'
import type { TelemetryStore } from '../TelemetryStore'

export type DongleAdapterDeps = {
  getDongleDriver: () => DongleDriver | null
  store: TelemetryStore
}

export type DongleAdapterHandle = {
  off: () => void
  hydrate: () => void
}

export function attachDongleAdapter({
  store,
  getDongleDriver
}: DongleAdapterDeps): DongleAdapterHandle {
  let lastNightMode: boolean | undefined

  const onChange = (patch: TelemetryPayload, snap: TelemetryPayload): void => {
    const dongle = getDongleDriver()
    if (!dongle) return

    // ── nightMode (diff-suppressed) ──────────────────────────────────
    if (
      'nightMode' in patch &&
      isWired('dongle', 'nightMode') &&
      typeof patch.nightMode === 'boolean'
    ) {
      if (patch.nightMode !== lastNightMode) {
        lastNightMode = patch.nightMode
        void dongle.send(new SendBoolean(patch.nightMode, FileAddress.NIGHT_MODE))
      }
    }

    // ── GPS / GNSS — emit on every gps patch (producer rate-limits) ──
    if ('gps' in patch && isWired('dongle', 'gps')) {
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
        if (nmea) void dongle.sendGnssData(nmea)
      }
    }
  }

  store.on('change', onChange)
  return {
    off: (): void => {
      store.off('change', onChange)
    },
    hydrate: (): void => {
      lastNightMode = undefined // reset so next push (incl. this hydration) re-fires
      const snap = store.snapshot()
      if (Object.keys(snap).length === 0) return
      onChange(snap, snap)
    }
  }
}
