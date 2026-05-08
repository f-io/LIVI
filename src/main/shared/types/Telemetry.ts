/* eslint-disable @typescript-eslint/no-explicit-any */
//
// ╔══════════════════════════════════════════════════════════════════════════╗
// ║  LIVI Telemetry API  —  USER → LIVI                                      ║
// ║  ─────────────────────────────────────────────────────────────────────   ║
// ║  This file is the single source of truth for what data LIVI accepts      ║
// ║  from the outside world (CAN bridges, OBD readers, GNSS modules, …) and  ║
// ║  where each field flows to internally.                                   ║
// ║                                                                          ║
// ║  Two things you'll find here:                                            ║
// ║    1. `TelemetryPayload`  — the typed contract (what you can send)       ║
// ║    2. `TELEMETRY_ROUTES`  — the routing table (what consumes what)       ║
// ║                                                                          ║
// ║  If you're integrating CAN data with LIVI, this file is the only one     ║
// ║  you need to read.                                                       ║
// ╚══════════════════════════════════════════════════════════════════════════╝
//
//
//  HOW TO PUSH DATA
//  ════════════════
//
//  Transport options (both expect the same JSON shape):
//
//    Socket.IO  ws://<livi-host>:4000   event: "telemetry:push"
//                 io('ws://livi.local:4000').emit('telemetry:push', { speedKph: 73 })
//
//    IPC        renderer-side
//                 window.api.pushTelemetry({ speedKph: 73 })
//
//  Granularity is up to you — LIVI merges every push into a central store
//  and re-distributes only the fields that changed. Send a single value or
//  a whole block, both work:
//
//    { speedKph: 73 }                                // single value
//    { speedKph: 73, rpm: 2100, gear: 'D' }          // small block
//    { gps: { lat: 52.5, lng: 13.4, heading: 90 } }  // grouped block
//    { speedKph: 73, gps: { lat: 52.5, lng: 13.4 },  // mixed
//      lights: true, turn: 'left' }
//
//  Nested blocks (`gps`, `can`) are shallow-merged on a per-field basis: a
//  push of `{ gps: { lat: 50 } }` keeps the previously known `lng`.
//
//
//  ROUTING OVERVIEW
//  ════════════════
//
//  Every field is routed by the table below. ✓ = receiver consumes the
//  field, · = ignored, TODO = wiring still missing on that side.
//
//    Field                          Dash  AA-native  Dongle
//    ─────────────────────────────────────────────────────
//    speedKph                        ✓     ✓          ·
//    rpm                             ✓     ✓          ·
//    gear                            ✓     ✓          ·
//    reverse                         ✓     ✓ (gear)   ·
//    steeringDeg                     ✓     ·          ·
//    turn (blinker)                  ✓     ✓          ·
//    lights / highBeam / hazards     ✓     ✓          ·
//    parkingBrake                    ✓     ✓          ·
//    nightMode                       ✓     ✓          ✓
//    fuelPct / rangeKm               ✓     ✓          ·
//    fuelRateLph / consumption*      ✓     ·          ·
//    batteryCapacityKwh / Lvl        ·     ✓ (VEM)    ·
//    coolantC / oilC / iatC          ✓     ·          ·
//    transmissionC                   ✓     ·          ·
//    ambientC                        ✓     ✓          ·
//    baroKpa                         ✓     ✓          ·
//    mapKpa / boostKpa               ✓     ·          ·
//    lambda / afr                    ✓     ·          ·
//    batteryV                        ✓     ·          ·
//    ambientLux                      ✓     ·          ·
//    odometerKm / odometerTripKm     ✓     ✓          ·
//    drivingStatus                   ✓     ✓          ·
//    gps.lat / lng / alt / heading   ✓     ✓          ✓
//    gps.speedMs / accuracyM         ✓     ✓          ·
//    can { id, data, bus }           ✓     ·          ·
//
//  The dongle is a CarPlay/AA bridge box, not a vehicle bus — the only
//  vehicle-side data it consumes are GPS (NMEA over `SendGnssData`) and the
//  night-mode boolean. Everything else is HU-side.
//
//
//  ADDING NEW FIELDS
//  ═════════════════
//
//  1. Add the field to `TelemetryPayload` below with a JSDoc unit comment.
//  2. Add a row to `TELEMETRY_ROUTES` declaring receivers.
//  3. Implement consumption in the relevant adapter(s) under
//     `src/main/services/telemetry/adapters/`.
//  4. Update the routing-overview table above
//

// ──────────────────────────────────────────────────────────────────────────
// Sub-blocks
// ──────────────────────────────────────────────────────────────────────────

/**
 * GPS / GNSS. Send the whole block on every fix, or partial updates if
 * a sensor only delivers a subset (e.g. dead-reckoning without accuracy).
 */
export type GpsPayload = {
  /** Latitude in decimal degrees (WGS84). Required for AA / Dongle to use it. */
  lat?: number
  /** Longitude in decimal degrees (WGS84). Required for AA / Dongle to use it. */
  lng?: number
  /** Altitude in meters above sea level. */
  alt?: number
  /** Heading / bearing in degrees (0 = north, clockwise). */
  heading?: number
  /** Ground speed in meters per second. AA prefers this over `speedKph`. */
  speedMs?: number
  /** Horizontal accuracy in meters (smaller = better). */
  accuracyM?: number
  /** Number of satellites in the fix. Dash-only. */
  satellites?: number
  /** Unix-ms timestamp of the fix (defaults to ingest time). */
  fixTs?: number
}

/** Raw CAN frame passthrough — for tooling/diagnostics, not for receivers. */
export type CanFrame = {
  id: number
  data: number[]
  bus?: number
}

// ──────────────────────────────────────────────────────────────────────────
// Main payload
// ──────────────────────────────────────────────────────────────────────────

export type TelemetryPayload = {
  /** Unix-ms timestamp of the producer. LIVI fills this on ingest if absent. */
  ts?: number

  // ── Vehicle motion / cluster basics ────────────────────────────────────

  /** Vehicle speed in km/h. */
  speedKph?: number
  /** Engine speed in RPM. */
  rpm?: number
  /** Gear indicator. Number for manual/DSG: -1=R, 0=N, 1..10=M1..M10.
   *  String for automatic: 'P' | 'R' | 'N' | 'D' | 'S' | 'M1'..'M10'. */
  gear?: number | string
  /** Steering wheel angle in degrees (negative = left). */
  steeringDeg?: number

  // ── Driver-facing booleans ─────────────────────────────────────────────

  /** Reverse engaged. AA derives this from `gear` if the gear flag is sent. */
  reverse?: boolean
  /** Low-beam headlight on/off. */
  lights?: boolean
  /** High-beam. When `true`, AA head-light state goes to HIGH regardless of `lights`. */
  highBeam?: boolean
  /** Hazard lights on/off. */
  hazards?: boolean
  /** Turn indicator state. */
  turn?: 'none' | 'left' | 'right'
  /** Parking brake engaged. */
  parkingBrake?: boolean

  // ── Temperatures (°C) ──────────────────────────────────────────────────

  /** Engine coolant temperature. */
  coolantC?: number
  /** Engine oil temperature. */
  oilC?: number
  /** Automatic-transmission oil temperature. */
  transmissionC?: number
  /** Intake air temperature. */
  iatC?: number
  /** Outside / ambient temperature. AA also forwards this to its env channel. */
  ambientC?: number

  // ── Electrical ─────────────────────────────────────────────────────────

  /** 12V battery voltage. */
  batteryV?: number

  // ── Fuel / consumption / range ─────────────────────────────────────────

  /** Fuel level in % (0..100). For EVs: state-of-charge (SoC). */
  fuelPct?: number
  /** Remaining range in km. */
  rangeKm?: number
  /** Instant fuel rate in L/h. */
  fuelRateLph?: number
  /** Instant consumption in L/100km (momentary). */
  consumptionLPer100Km?: number
  /** Average consumption in L/100km. */
  consumptionAvgLPer100Km?: number

  // ── EV battery  ─────────────────────────────────────────

  /** Gross battery capacity in kWh. Required for AA's VehicleEnergyModel. */
  batteryCapacityKwh?: number
  /** Current battery level in kWh. Derived from `fuelPct × capacity` if absent. */
  batteryLevelKwh?: number

  // ── Engine air / boost / fueling ───────────────────────────────────────

  /** Manifold absolute pressure in kPa (MAP). */
  mapKpa?: number
  /** Barometric / ambient pressure in kPa. AA forwards this on env channel. */
  baroKpa?: number
  /** Boost pressure in kPa. */
  boostKpa?: number
  /** Lambda (equivalence ratio). 1.0 = stoichiometric. */
  lambda?: number
  /** Air-fuel ratio. Optional alternative to lambda. */
  afr?: number

  // ── Distance / driving status ──────────────────────────────────────────

  /** Total odometer in km. AA receives this with 0.1 km resolution. */
  odometerKm?: number
  /** Trip odometer in km. */
  odometerTripKm?: number
  /** AA driving-status restriction bitmask. 0 = unrestricted. Pass-through for AA. */
  drivingStatus?: number

  // ── Environment / sensors ──────────────────────────────────────────────

  /** Ambient light sensor value in lux (raw). */
  ambientLux?: number

  // ── External UI overrides ──────────────────────────────────────────────

  /** Force night mode for LIVI UI / AA / Dongle regardless of ambient sensor. */
  nightMode?: boolean

  // ── GNSS sub-block ─────────────────────────────────────────────────────

  /** GPS / GNSS fix data. See `GpsPayload`. */
  gps?: GpsPayload

  // ── Raw CAN frame passthrough ──────────────────────────────────────────

  /** Pass a raw CAN frame through to the renderer for tooling / sniffing. */
  can?: CanFrame

  // Extension point: allow experimentation without changing types.
  [key: string]: unknown
}

// ──────────────────────────────────────────────────────────────────────────
// Routing registry
// ──────────────────────────────────────────────────────────────────────────

/** A telemetry sink. Adapters identify themselves with one of these. */
export type TelemetryReceiver = 'dash' | 'aa' | 'dongle'

/** Routing flag value: receiver consumes the field, doesn't, or has wiring TODO. */
export type RouteFlag = true | false | 'TODO'

export type TelemetryRoute = Readonly<Record<TelemetryReceiver, RouteFlag>>

/**
 * The single source of truth for which receiver consumes which field.
 *
 * Adapters MUST consult this table before pushing — `if (!routes(key).aa)
 * return` keeps the routing decision in one place. New fields without an
 * entry here will produce a TypeScript error in `Record<keyof T>`.
 *
 * `'TODO'` means the field is intended for that receiver but the adapter
 * isn't wired yet — treat it as `false` at runtime.
 */
export const TELEMETRY_ROUTES = {
  // Motion / cluster basics
  speedKph: { dash: true, aa: true, dongle: false },
  rpm: { dash: true, aa: true, dongle: false },
  gear: { dash: true, aa: true, dongle: false },
  steeringDeg: { dash: true, aa: false, dongle: false },

  // Driver-facing booleans
  reverse: { dash: true, aa: true, dongle: false },
  lights: { dash: true, aa: true, dongle: false },
  highBeam: { dash: true, aa: true, dongle: false },
  hazards: { dash: true, aa: true, dongle: false },
  turn: { dash: true, aa: true, dongle: false },
  parkingBrake: { dash: true, aa: true, dongle: false },

  // Temperatures
  coolantC: { dash: true, aa: false, dongle: false },
  oilC: { dash: true, aa: false, dongle: false },
  transmissionC: { dash: true, aa: false, dongle: false },
  iatC: { dash: true, aa: false, dongle: false },
  ambientC: { dash: true, aa: true, dongle: false },

  // Electrical
  batteryV: { dash: true, aa: false, dongle: false },

  // Fuel / consumption / range
  fuelPct: { dash: true, aa: true, dongle: false },
  rangeKm: { dash: true, aa: true, dongle: false },
  fuelRateLph: { dash: true, aa: false, dongle: false },
  consumptionLPer100Km: { dash: true, aa: false, dongle: false },
  consumptionAvgLPer100Km: { dash: true, aa: false, dongle: false },
  batteryCapacityKwh: { dash: false, aa: true, dongle: false },
  batteryLevelKwh: { dash: false, aa: true, dongle: false },

  // Engine air / boost / fueling
  mapKpa: { dash: true, aa: false, dongle: false },
  baroKpa: { dash: true, aa: true, dongle: false },
  boostKpa: { dash: true, aa: false, dongle: false },
  lambda: { dash: true, aa: false, dongle: false },
  afr: { dash: true, aa: false, dongle: false },

  // Distance / driving status
  odometerKm: { dash: true, aa: true, dongle: false },
  odometerTripKm: { dash: true, aa: true, dongle: false },
  drivingStatus: { dash: true, aa: true, dongle: false },

  // Environment
  ambientLux: { dash: true, aa: false, dongle: false },

  // External overrides
  nightMode: { dash: true, aa: true, dongle: true },

  // GNSS — whole sub-block consumed; per-field availability documented in GpsPayload.
  gps: { dash: true, aa: true, dongle: true },

  // Raw passthrough
  can: { dash: true, aa: false, dongle: false },

  // Producer timestamp — purely metadata
  ts: { dash: true, aa: false, dongle: false }
} as const satisfies Record<keyof TelemetryPayload, TelemetryRoute>

/** Look up a routing entry; safe for the open `[key: string]: unknown` extension point. */
export function routes(key: string): TelemetryRoute {
  const entry = (TELEMETRY_ROUTES as Record<string, TelemetryRoute | undefined>)[key]
  return entry ?? { dash: false, aa: false, dongle: false }
}

/** Resolve `'TODO'` to runtime-effective `false`. */
export function isWired(receiver: TelemetryReceiver, key: string): boolean {
  return routes(key)[receiver] === true
}
