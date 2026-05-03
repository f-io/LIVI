import type { AaDriver } from '@projection/driver/aa/aaDriver'
import type { TelemetryPayload } from '@shared/types/Telemetry'

/** Map TelemetryPayload.gear (string|number) + reverse-flag → AA Gear enum. */
export function mapGearToAa(
  gear: string | number | undefined,
  reverseFlag: boolean | undefined
): number | undefined {
  if (typeof gear === 'number') {
    if (gear === -1) return 102 // REVERSE
    if (gear === 0) return 0 // NEUTRAL
    if (gear >= 1 && gear <= 10) return gear
  } else if (typeof gear === 'string') {
    const g = gear.trim().toUpperCase()
    if (g === 'P') return 101
    if (g === 'R') return 102
    if (g === 'N') return 0
    if (g === 'D' || g === 'S') return 100
    const m = /^M(\d{1,2})$/.exec(g)
    if (m) {
      const n = Number(m[1])
      if (n >= 1 && n <= 10) return n
    }
  }
  if (reverseFlag === true) return 102
  return undefined
}

/**
 * Forward the fields we know how to encode from a TelemetryPayload to AA's
 * sensor channel. Unset fields are left untouched on the phone.
 */
export function pushTelemetryToAa(aa: AaDriver, payload: TelemetryPayload): void {
  if (payload.fuelPct !== undefined || payload.rangeKm !== undefined) {
    const level =
      typeof payload.fuelPct === 'number'
        ? Math.max(0, Math.min(100, Math.round(payload.fuelPct)))
        : 0
    const rangeM =
      typeof payload.rangeKm === 'number'
        ? Math.max(0, Math.round(payload.rangeKm * 1000))
        : undefined
    const lowFuel = typeof payload.fuelPct === 'number' ? payload.fuelPct < 10 : undefined
    aa.sendFuelData(level, rangeM, lowFuel)
  }

  if (typeof payload.speedKph === 'number') {
    aa.sendSpeedData(Math.max(0, Math.round((payload.speedKph * 1000) / 3.6)))
  }

  if (typeof payload.rpm === 'number') {
    aa.sendRpmData(Math.max(0, Math.round(payload.rpm * 1000)))
  }

  const gearEnum = mapGearToAa(payload.gear, payload.reverse)
  if (gearEnum !== undefined) aa.sendGearData(gearEnum)

  if (typeof payload.nightMode === 'boolean') aa.sendNightModeData(payload.nightMode)

  if (typeof payload.lights === 'boolean') aa.sendLightData(payload.lights ? 2 : 1)

  if (typeof payload.ambientC === 'number' || typeof payload.baroKpa === 'number') {
    const tempE3 =
      typeof payload.ambientC === 'number' ? Math.round(payload.ambientC * 1000) : undefined
    const pressE3 =
      typeof payload.baroKpa === 'number' ? Math.round(payload.baroKpa * 1000) : undefined
    aa.sendEnvironmentData(tempE3, pressE3)
  }
}
