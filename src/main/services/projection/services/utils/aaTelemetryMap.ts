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

  // Lights / hazards / indicators — all share LightData.
  // head_light_state: 1=OFF, 2=ON, 3=HIGH. highBeam wins over plain `lights`.
  if (
    typeof payload.lights === 'boolean' ||
    typeof payload.highBeam === 'boolean' ||
    typeof payload.hazards === 'boolean' ||
    payload.turn !== undefined
  ) {
    let head: 1 | 2 | 3 | undefined
    if (payload.highBeam === true) head = 3
    else if (payload.lights === true) head = 2
    else if (payload.lights === false) head = 1
    const turn =
      payload.turn === 'left'
        ? 2
        : payload.turn === 'right'
          ? 3
          : payload.turn === 'none'
            ? 1
            : undefined
    aa.sendLightData(head, payload.hazards, turn)
  }

  if (typeof payload.parkingBrake === 'boolean') aa.sendParkingBrakeData(payload.parkingBrake)

  // EV VehicleEnergyModel — only when battery info is provided.
  // Maps' EV range bar + low-range warning depends on this.
  if (
    typeof payload.batteryCapacityKwh === 'number' &&
    typeof payload.rangeKm === 'number' &&
    payload.rangeKm > 0
  ) {
    const capacityWh = Math.round(payload.batteryCapacityKwh * 1000)
    const currentWh =
      typeof payload.batteryLevelKwh === 'number'
        ? Math.round(payload.batteryLevelKwh * 1000)
        : typeof payload.fuelPct === 'number'
          ? Math.round((payload.fuelPct / 100) * capacityWh)
          : 0
    if (capacityWh > 0 && currentWh > 0) {
      const rangeM = Math.round(payload.rangeKm * 1000)
      aa.sendVehicleEnergyModel(capacityWh, currentWh, rangeM)
    }
  }

  if (typeof payload.ambientC === 'number' || typeof payload.baroKpa === 'number') {
    const tempE3 =
      typeof payload.ambientC === 'number' ? Math.round(payload.ambientC * 1000) : undefined
    const pressE3 =
      typeof payload.baroKpa === 'number' ? Math.round(payload.baroKpa * 1000) : undefined
    aa.sendEnvironmentData(tempE3, pressE3)
  }
}
