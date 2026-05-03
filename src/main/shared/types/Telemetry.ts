export type TelemetryPayload = {
  // Timestamp (unix ms)
  ts?: number

  // ────────────────────────────────────────────────────────────────────────────
  // Vehicle motion / driver-facing "cluster" basics
  // ────────────────────────────────────────────────────────────────────────────

  // Vehicle speed (km/h)
  speedKph?: number

  // Engine speed (RPM)
  rpm?: number

  // Gear indicator - manual/DSG/automatic: -1/0/1.. or "R"/"N"/"D"/"S"/"M1"
  gear?: number | string

  // Steering wheel angle in degrees
  steeringDeg?: number

  // Convenience booleans for UI state
  reverse?: boolean
  // Low-beam headlight on/off
  lights?: boolean
  // High-beam. When true, head-light state goes to HIGH regardless of `lights`.
  highBeam?: boolean
  // Hazard lights
  hazards?: boolean
  // Turn indicator: 'none' | 'left' | 'right'
  turn?: 'none' | 'left' | 'right'
  // Parking brake engaged
  parkingBrake?: boolean

  // ────────────────────────────────────────────────────────────────────────────
  // Temperatures
  // ────────────────────────────────────────────────────────────────────────────

  // Engine coolant temperature in °C
  coolantC?: number

  // Engine oil temperature in °C
  oilC?: number

  // Automatic transmission oil temperature in °C
  transmissionC?: number

  // Intake air temperature in °C
  iatC?: number

  // Ambient temperature in °C
  ambientC?: number

  // ────────────────────────────────────────────────────────────────────────────
  // Electrical / battery
  // ────────────────────────────────────────────────────────────────────────────

  // Battery voltage
  batteryV?: number

  // ────────────────────────────────────────────────────────────────────────────
  // Fuel / consumption / range (driver-facing)
  // ────────────────────────────────────────────────────────────────────────────

  // Fuel level in percent (0..100). For EVs: state of charge (SoC).
  fuelPct?: number

  // Remaining range in km
  rangeKm?: number

  // ── EV battery (only meaningful when carType is Electric / Hybrid) ────────
  // Gross battery capacity in kWh (e.g. 50 = 50 kWh). When set together with
  // fuelPct + rangeKm, LIVI also pushes a VehicleEnergyModel sensor batch
  // (Maps' EV range display + low-range warning rely on this).
  batteryCapacityKwh?: number
  // Current battery level in kWh. Optional — derived from fuelPct × capacity
  // when omitted.
  batteryLevelKwh?: number

  // Instant fuel rate in liters per hour (L/h)
  fuelRateLph?: number

  // Instant consumption in L/100km (momentary)
  consumptionLPer100Km?: number

  // Average consumption in L/100km
  consumptionAvgLPer100Km?: number

  // ────────────────────────────────────────────────────────────────────────────
  // Engine air / boost / fueling
  // ────────────────────────────────────────────────────────────────────────────

  // Manifold absolute pressure in kPa (MAP)
  mapKpa?: number

  // Barometric / ambient pressure in kPa
  baroKpa?: number

  // Boost pressure in kPa
  boostKpa?: number

  // Lambda (equivalence ratio). 1.0 = stoichiometric
  lambda?: number

  // AFR (air-fuel ratio). Optional alternative display to lambda
  afr?: number

  // ────────────────────────────────────────────────────────────────────────────
  // Environment / sensors
  // ────────────────────────────────────────────────────────────────────────────

  // Ambient light sensor (lux)
  ambientLux?: number

  // ────────────────────────────────────────────────────────────────────────────
  // External control / UI overrides
  // ────────────────────────────────────────────────────────────────────────────

  // Appearance override for LIVI UI and Phone
  nightMode?: boolean

  // ────────────────────────────────────────────────────────────────────────────
  // Raw CAN frame passthrough
  // ────────────────────────────────────────────────────────────────────────────

  can?: { id: number; data: number[]; bus?: number }

  // ────────────────────────────────────────────────────────────────────────────
  // Extension point: allow experimentation without changing types
  // ────────────────────────────────────────────────────────────────────────────
  [key: string]: unknown
}
