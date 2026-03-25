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
  lights?: boolean

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

  // Fuel level in percent (0..100)
  fuelPct?: number

  // Remaining range in km
  rangeKm?: number

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
