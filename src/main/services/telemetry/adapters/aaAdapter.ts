/**
 * Android Auto telemetry adapter.
 *
 * Subscribes to `TelemetryStore.change` and forwards each field that
 * `TELEMETRY_ROUTES[key].aa === true` to the active AA driver.
 *
 * Bundled fields:
 *   - lights / highBeam / hazards / turn   → one `sendLightData` call
 *   - fuelPct / rangeKm                    → one `sendFuelData` call
 *   - ambientC / baroKpa                   → one `sendEnvironmentData` call
 *   - odometerKm / odometerTripKm          → one `sendOdometerData` call
 *   - gps.*                                → one `sendGpsLocationData` call
 *   - batteryCapacityKwh / Lvl / range     → one (throttled) VEM call
 *
 */

import type { AaDriver } from '@projection/driver/aa/aaDriver'
import { type GpsPayload, isWired, type TelemetryPayload } from '@shared/types/Telemetry'
import type { TelemetryStore } from '../TelemetryStore'

// Maps re-routes lazily on VEM updates
const VEM_MIN_INTERVAL_MS = 10_000

export type AaAdapterDeps = {
  getAaDriver: () => AaDriver | null
  store: TelemetryStore
}

export type AaAdapterHandle = {
  off: () => void
  hydrate: () => void
}

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

export function attachAaAdapter({ store, getAaDriver }: AaAdapterDeps): AaAdapterHandle {
  // Per-key diff cache. We compare on the value the AA driver actually
  // receives (post-conversion) — that way `73.4 km/h → 73 km/h` doesn't
  // trigger a fresh push when the producer sends 73.4 every tick.
  type Cache = {
    speedMmS?: number
    rpmE3?: number
    gearEnum?: number
    nightMode?: boolean
    parkingBrake?: boolean
    headLight?: 1 | 2 | 3 | undefined
    turn?: 1 | 2 | 3 | undefined
    hazards?: boolean
    fuelLevel?: number
    fuelRangeM?: number
    fuelLow?: boolean
    odometerKmE1?: number
    odometerTripKmE1?: number
    drivingStatus?: number
    envTempE3?: number
    envPressE3?: number
    gpsLat?: number
    gpsLng?: number
    gpsAccuracyM?: number
    gpsAltitudeM?: number
    gpsSpeedMs?: number
    gpsBearingDeg?: number
    vemSentAt?: number
  }
  let cache: Cache = {}

  const onChange = (patch: TelemetryPayload, snap: TelemetryPayload): void => {
    const aa = getAaDriver()
    if (!aa) return

    // ── Single-field forwarders ────────────────────────────────────────

    if ('speedKph' in patch && isWired('aa', 'speedKph') && typeof patch.speedKph === 'number') {
      const v = Math.max(0, Math.round((patch.speedKph * 1000) / 3.6))
      if (v !== cache.speedMmS) {
        cache.speedMmS = v
        aa.sendSpeedData(v)
      }
    }

    if ('rpm' in patch && isWired('aa', 'rpm') && typeof patch.rpm === 'number') {
      const v = Math.max(0, Math.round(patch.rpm * 1000))
      if (v !== cache.rpmE3) {
        cache.rpmE3 = v
        aa.sendRpmData(v)
      }
    }

    if (('gear' in patch || 'reverse' in patch) && isWired('aa', 'gear')) {
      const enum_ = mapGearToAa(snap.gear, snap.reverse)
      if (enum_ !== undefined && enum_ !== cache.gearEnum) {
        cache.gearEnum = enum_
        aa.sendGearData(enum_)
      }
    }

    if (
      'nightMode' in patch &&
      isWired('aa', 'nightMode') &&
      typeof patch.nightMode === 'boolean'
    ) {
      if (patch.nightMode !== cache.nightMode) {
        cache.nightMode = patch.nightMode
        aa.sendNightModeData(patch.nightMode)
      }
    }

    if (
      'parkingBrake' in patch &&
      isWired('aa', 'parkingBrake') &&
      typeof patch.parkingBrake === 'boolean'
    ) {
      if (patch.parkingBrake !== cache.parkingBrake) {
        cache.parkingBrake = patch.parkingBrake
        aa.sendParkingBrakeData(patch.parkingBrake)
      }
    }

    if (
      'drivingStatus' in patch &&
      isWired('aa', 'drivingStatus') &&
      typeof patch.drivingStatus === 'number'
    ) {
      if (patch.drivingStatus !== cache.drivingStatus) {
        cache.drivingStatus = patch.drivingStatus
        aa.sendDrivingStatusData(patch.drivingStatus)
      }
    }

    // ── Bundled: lights / highBeam / hazards / turn ───────────────────
    // head_light_state: 1=OFF, 2=ON, 3=HIGH. highBeam wins over `lights`.

    if ('lights' in patch || 'highBeam' in patch || 'hazards' in patch || 'turn' in patch) {
      let head: 1 | 2 | 3 | undefined
      if (snap.highBeam === true) head = 3
      else if (snap.lights === true) head = 2
      else if (snap.lights === false) head = 1
      const turn =
        snap.turn === 'left' ? 2 : snap.turn === 'right' ? 3 : snap.turn === 'none' ? 1 : undefined
      const hazards = typeof snap.hazards === 'boolean' ? snap.hazards : undefined

      if (head !== cache.headLight || turn !== cache.turn || hazards !== cache.hazards) {
        cache.headLight = head
        cache.turn = turn
        cache.hazards = hazards
        aa.sendLightData(head, hazards, turn)
      }
    }

    // ── Bundled: fuel level / range ───────────────────────────────────

    if (
      ('fuelPct' in patch || 'rangeKm' in patch) &&
      (isWired('aa', 'fuelPct') || isWired('aa', 'rangeKm'))
    ) {
      const level =
        typeof snap.fuelPct === 'number'
          ? Math.max(0, Math.min(100, Math.round(snap.fuelPct)))
          : undefined
      const rangeM =
        typeof snap.rangeKm === 'number' ? Math.max(0, Math.round(snap.rangeKm * 1000)) : undefined
      const lowFuel = typeof snap.fuelPct === 'number' ? snap.fuelPct < 10 : undefined

      if (
        level !== undefined &&
        (level !== cache.fuelLevel || rangeM !== cache.fuelRangeM || lowFuel !== cache.fuelLow)
      ) {
        cache.fuelLevel = level
        cache.fuelRangeM = rangeM
        cache.fuelLow = lowFuel
        aa.sendFuelData(level, rangeM, lowFuel)
      }
    }

    // ── Bundled: odometer ─────────────────────────────────────────────

    if (('odometerKm' in patch || 'odometerTripKm' in patch) && isWired('aa', 'odometerKm')) {
      const total =
        typeof snap.odometerKm === 'number' ? Math.round(snap.odometerKm * 10) : undefined
      const trip =
        typeof snap.odometerTripKm === 'number' ? Math.round(snap.odometerTripKm * 10) : undefined
      if (
        total !== undefined &&
        (total !== cache.odometerKmE1 || trip !== cache.odometerTripKmE1)
      ) {
        cache.odometerKmE1 = total
        cache.odometerTripKmE1 = trip
        aa.sendOdometerData(total, trip)
      }
    }

    // ── Bundled: ambient temperature / barometric pressure ────────────

    if (
      ('ambientC' in patch || 'baroKpa' in patch) &&
      (isWired('aa', 'ambientC') || isWired('aa', 'baroKpa'))
    ) {
      const tempE3 =
        typeof snap.ambientC === 'number' ? Math.round(snap.ambientC * 1000) : undefined
      const pressE3 = typeof snap.baroKpa === 'number' ? Math.round(snap.baroKpa * 1000) : undefined
      if (tempE3 !== cache.envTempE3 || pressE3 !== cache.envPressE3) {
        cache.envTempE3 = tempE3
        cache.envPressE3 = pressE3
        aa.sendEnvironmentData(tempE3, pressE3)
      }
    }

    // ── GPS / GNSS ────────────────────────────────────────────────────

    if ('gps' in patch && isWired('aa', 'gps')) {
      const gps = (snap.gps ?? {}) as GpsPayload
      const lat = typeof gps.lat === 'number' ? gps.lat : undefined
      const lng = typeof gps.lng === 'number' ? gps.lng : undefined
      // AA proto requires both lat + lng on every LocationData.
      if (lat !== undefined && lng !== undefined) {
        const changed =
          lat !== cache.gpsLat ||
          lng !== cache.gpsLng ||
          gps.accuracyM !== cache.gpsAccuracyM ||
          gps.alt !== cache.gpsAltitudeM ||
          gps.speedMs !== cache.gpsSpeedMs ||
          gps.heading !== cache.gpsBearingDeg
        if (changed) {
          cache.gpsLat = lat
          cache.gpsLng = lng
          cache.gpsAccuracyM = gps.accuracyM
          cache.gpsAltitudeM = gps.alt
          cache.gpsSpeedMs = gps.speedMs
          cache.gpsBearingDeg = gps.heading
          aa.sendGpsLocationData({
            latDeg: lat,
            lngDeg: lng,
            accuracyM: gps.accuracyM,
            altitudeM: gps.alt,
            speedMs: gps.speedMs,
            bearingDeg: gps.heading
          })
        }
      }
    }

    // ── EV VehicleEnergyModel — throttled, requires capacity + range ──

    if (
      typeof snap.batteryCapacityKwh === 'number' &&
      typeof snap.rangeKm === 'number' &&
      snap.rangeKm > 0
    ) {
      const now = Date.now()
      if (!cache.vemSentAt || now - cache.vemSentAt >= VEM_MIN_INTERVAL_MS) {
        const capacityWh = Math.round(snap.batteryCapacityKwh * 1000)
        const currentWh =
          typeof snap.batteryLevelKwh === 'number'
            ? Math.round(snap.batteryLevelKwh * 1000)
            : typeof snap.fuelPct === 'number'
              ? Math.round((snap.fuelPct / 100) * capacityWh)
              : 0
        if (capacityWh > 0 && currentWh > 0) {
          const rangeM = Math.round(snap.rangeKm * 1000)
          aa.sendVehicleEnergyModel(capacityWh, currentWh, rangeM)
          cache.vemSentAt = now
        }
      }
    }
  }

  store.on('change', onChange)
  return {
    off: (): void => {
      store.off('change', onChange)
    },
    hydrate: (): void => {
      // Reset the diff cache so every bundle re-fires for the new session.
      cache = {}
      const snap = store.snapshot()
      if (Object.keys(snap).length === 0) return
      // Drive onChange as if every field had just been pushed: the snapshot
      // serves as both `patch` and `snap` so the `'foo' in patch` guards
      // pick up every populated key.
      onChange(snap, snap)
    }
  }
}
