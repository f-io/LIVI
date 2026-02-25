import { Server } from 'socket.io'
import { EventEmitter } from 'events'
import http from 'http'

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
  // Raw CAN frame passthrough
  // ────────────────────────────────────────────────────────────────────────────

  can?: { id: number; data: number[]; bus?: number }

  // ────────────────────────────────────────────────────────────────────────────
  // Extension point: allow experimentation without changing types
  // ────────────────────────────────────────────────────────────────────────────
  [key: string]: unknown
}

export enum TelemetryEvents {
  Connection = 'connection',

  // external -> main
  Push = 'telemetry:push',

  // main -> clients
  Update = 'telemetry:update',
  Reverse = 'telemetry:reverse',
  Lights = 'telemetry:lights'
}

export class TelemetrySocket extends EventEmitter {
  io: Server | null = null
  httpServer: http.Server | null = null

  private last: TelemetryPayload | null = null
  private lastReverse: boolean | null = null
  private lastLights: boolean | null = null

  constructor(private port = 4000) {
    super()
    this.startServer()
  }

  private setupListeners() {
    this.io?.on(TelemetryEvents.Connection, (socket) => {
      if (this.last) {
        socket.emit(TelemetryEvents.Update, this.last)
      }

      socket.on(TelemetryEvents.Push, (payload: TelemetryPayload) => {
        this.emit(TelemetryEvents.Push, payload)
        this.publishTelemetry(payload)
      })
    })
  }

  private startServer() {
    this.httpServer = http.createServer()
    this.io = new Server(this.httpServer, { cors: { origin: '*' } })
    this.setupListeners()
    this.httpServer.listen(this.port, () => {
      console.log(`[TelemetrySocket] Server listening on port ${this.port}`)
    })
  }

  async disconnect(): Promise<void> {
    return new Promise((resolve) => {
      if (this.io) this.io.close(() => console.log('[TelemetrySocket] IO closed'))
      if (this.httpServer) {
        this.httpServer.close(() => {
          console.log('[TelemetrySocket] HTTP server closed')
          this.io = null
          this.httpServer = null
          resolve()
        })
      } else {
        resolve()
      }
    })
  }

  async connect(): Promise<void> {
    await new Promise((r) => setTimeout(r, 200))
    this.startServer()
  }

  // main -> all clients
  publishTelemetry(payload: TelemetryPayload) {
    const msg: TelemetryPayload = { ts: Date.now(), ...payload }

    this.last = msg
    this.io?.emit(TelemetryEvents.Update, msg)

    const reverse =
      typeof msg.reverse === 'boolean' ? msg.reverse : msg.gear === 'R' || msg.gear === -1

    if (typeof reverse === 'boolean' && reverse !== this.lastReverse) {
      this.lastReverse = reverse
      this.io?.emit(TelemetryEvents.Reverse, reverse)
    }

    if (typeof msg.lights === 'boolean' && msg.lights !== this.lastLights) {
      this.lastLights = msg.lights
      this.io?.emit(TelemetryEvents.Lights, msg.lights)
    }
  }

  publishReverse(reverse: boolean) {
    if (reverse !== this.lastReverse) {
      this.lastReverse = reverse
      if (this.last) this.last = { ...this.last, reverse }
    }
    this.io?.emit(TelemetryEvents.Reverse, reverse)
  }

  publishLights(lights: boolean) {
    if (lights !== this.lastLights) {
      this.lastLights = lights
      if (this.last) this.last = { ...this.last, lights }
    }
    this.io?.emit(TelemetryEvents.Lights, lights)
  }
}
