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
        const nmea = encodeNmea(gps.lat, gps.lng, gps.alt, gps.heading, gps.speedMs, gps.fixTs)
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

// ──────────────────────────────────────────────────────────────────────────
// NMEA-0183 encoder
// ──────────────────────────────────────────────────────────────────────────

/**
 * Build a `$GPGGA` + `$GPRMC` pair from a fix.
 *
 * Spec refs:
 *   GGA: hhmmss.ss,llll.ll,a,yyyyy.yy,a,x,xx,x.x,x.x,M,x.x,M,,
 *   RMC: hhmmss.ss,A,llll.ll,a,yyyyy.yy,a,x.x,x.x,ddmmyy,,,
 */
function encodeNmea(
  latDeg: number,
  lngDeg: number,
  altM: number | undefined,
  headingDeg: number | undefined,
  speedMs: number | undefined,
  fixTs: number | undefined
): string {
  const ts = fixTs && Number.isFinite(fixTs) ? new Date(fixTs) : new Date()
  const hh = pad2(ts.getUTCHours())
  const mm = pad2(ts.getUTCMinutes())
  const ss = pad2(ts.getUTCSeconds())
  const time = `${hh}${mm}${ss}.00`
  const date = `${pad2(ts.getUTCDate())}${pad2(ts.getUTCMonth() + 1)}${String(
    ts.getUTCFullYear()
  ).slice(-2)}`

  const { ddmm: latDdmm, hemi: latHemi } = degToNmea(latDeg, true)
  const { ddmm: lngDdmm, hemi: lngHemi } = degToNmea(lngDeg, false)

  // GGA: fix-quality=1 (GPS fix), satellites=8 (synthetic), HDOP=1.0
  const altStr = typeof altM === 'number' ? altM.toFixed(1) : '0.0'
  const ggaBody = `GPGGA,${time},${latDdmm},${latHemi},${lngDdmm},${lngHemi},1,08,1.0,${altStr},M,0.0,M,,`
  const gga = `$${ggaBody}*${nmeaChecksum(ggaBody)}`

  // RMC: status A (active). Speed in knots, course in degrees.
  const speedKn = typeof speedMs === 'number' ? (speedMs * 1.94384449).toFixed(2) : '0.00'
  const courseStr = typeof headingDeg === 'number' ? headingDeg.toFixed(2) : '0.00'
  const rmcBody = `GPRMC,${time},A,${latDdmm},${latHemi},${lngDdmm},${lngHemi},${speedKn},${courseStr},${date},,`
  const rmc = `$${rmcBody}*${nmeaChecksum(rmcBody)}`

  return `${gga}\r\n${rmc}\r\n`
}

function degToNmea(deg: number, isLat: boolean): { ddmm: string; hemi: string } {
  const abs = Math.abs(deg)
  const d = Math.floor(abs)
  const m = (abs - d) * 60
  // Latitude: ddmm.mmmm (2-digit deg). Longitude: dddmm.mmmm (3-digit deg).
  const dStr = isLat ? pad2(d) : pad3(d)
  const mStr = m.toFixed(4).padStart(7, '0') // mm.mmmm zero-padded
  const hemi = isLat ? (deg >= 0 ? 'N' : 'S') : deg >= 0 ? 'E' : 'W'
  return { ddmm: `${dStr}${mStr}`, hemi }
}

function nmeaChecksum(body: string): string {
  let cs = 0
  for (let i = 0; i < body.length; i++) cs ^= body.charCodeAt(i)
  return cs.toString(16).toUpperCase().padStart(2, '0')
}

function pad2(n: number): string {
  return String(n).padStart(2, '0')
}
function pad3(n: number): string {
  return String(n).padStart(3, '0')
}
