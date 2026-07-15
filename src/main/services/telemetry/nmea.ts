/**
 * NMEA-0183 encoder for a GPS fix — shared by the dongle and native-CarPlay adapters.
 *
 * Builds a `$GPGGA` + `$GPRMC` pair:
 *   GGA: hhmmss.ss,llll.ll,a,yyyyy.yy,a,x,xx,x.x,x.x,M,x.x,M,,
 *   RMC: hhmmss.ss,A,llll.ll,a,yyyyy.yy,a,x.x,x.x,ddmmyy,,,
 */
export function encodeNmea(
  latDeg: number,
  lngDeg: number,
  altM: number | undefined,
  headingDeg: number | undefined,
  speedMs: number | undefined,
  fixTs: number | undefined,
  accuracyM?: number
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

  // GGA: fix-quality=1 (GPS fix), satellites=8 (synthetic). HDOP carries the fix quality
  // so the device can weigh our fix against its own: derive it from horizontal accuracy
  // (~5 m per HDOP unit), default 1.0 when accuracy is unknown.
  const hdop =
    typeof accuracyM === 'number' && accuracyM > 0
      ? Math.min(50, Math.max(0.5, accuracyM / 5))
      : 1.0
  const altStr = typeof altM === 'number' ? altM.toFixed(1) : '0.0'
  const ggaBody = `GPGGA,${time},${latDdmm},${latHemi},${lngDdmm},${lngHemi},1,08,${hdop.toFixed(1)},${altStr},M,0.0,M,,`
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
