import { io } from 'socket.io-client'

const URL = process.env.TELEMETRY_URL ?? 'http://127.0.0.1:4000'
const SOURCE = process.env.TELEMETRY_SOURCE ?? 'sim'

const cmd = process.argv[2] ?? 'cycle'

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))
const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))
const lerp = (a: number, b: number, t: number) => a + (b - a) * t
const smoothstep = (t: number) => t * t * (3 - 2 * t)

// ─────────────────────────────────────────────────────────────────────────────
// 6-speed gear model (D) using typical ratios
// RPM derived from: wheelRPM * finalDrive * gearRatio
// wheelRPM derived from: speed / tireCircumference
// ─────────────────────────────────────────────────────────────────────────────

const MAX_RPM = 5500
const SHIFT_UP_RPM = 5000

type DriveGear = 1 | 2 | 3 | 4 | 5 | 6

const tireCircM = 2.0

// Targets for calibration
const TOP_SPEED_KPH = 225
const TOP_RPM_AT_TOP_SPEED = 5000

// Gearbox ratios
const ratios: Record<DriveGear, number> = {
  1: 3.77,
  2: 2.09,
  3: 1.32,
  4: 0.97,
  5: 0.76,
  6: 0.62
}

function clampGear(g: number): DriveGear {
  if (g <= 1) return 1
  if (g >= 6) return 6
  return g as DriveGear
}

function wheelRpmFromSpeed(speedKph: number) {
  // speed (m/min) = kph * 1000 / 60
  const mPerMin = (speedKph * 1000) / 60
  return mPerMin / tireCircM
}

// Calibrate final drive so that:
const finalDrive = TOP_RPM_AT_TOP_SPEED / (wheelRpmFromSpeed(TOP_SPEED_KPH) * ratios[6])

function rpmFromSpeedAndGear(speedKph: number, gear: DriveGear) {
  return wheelRpmFromSpeed(speedKph) * finalDrive * ratios[gear]
}

function connect() {
  const socket = io(URL, { transports: ['websocket'] })

  socket.on('connect', () => {
    console.log(`[telemetry-sim] connected ${socket.id} -> ${URL} (source=${SOURCE})`)
  })

  socket.on('connect_error', (e) => {
    console.error('[telemetry-sim] connect_error:', e?.message ?? e)
  })

  return socket
}

function push(socket: any, payload: Record<string, unknown>) {
  socket.emit('telemetry:push', { ts: Date.now(), source: SOURCE, ...payload })
}

// ─────────────────────────────────────────────────────────────────────────────
// Simple “vehicle” state + derived signals
// ─────────────────────────────────────────────────────────────────────────────

type Shifter = 'P' | 'N' | 'D' | 'R'
type DriveMode = 'D' | 'M'

type SimState = {
  // UI / IPC display value:
  // - 'P' | 'N' | 'R' when not driving
  // - 1..6 when in D (see publish below)
  gear: Shifter | `${'D' | 'M'}${DriveGear}`

  // internal shifter state
  shifter: Shifter

  // internal driving gear (1..6) while in D
  driveGear: DriveGear
  driveMode: DriveMode

  speedKph: number
  rpm: number

  coolantC: number
  oilC: number
  transmissionC: number
  iatC: number
  ambientC: number

  fuelPct: number
  rangeKm: number
  fuelRateLph: number
  consumptionLPer100Km: number
  consumptionAvgLPer100Km: number

  batteryV: number

  mapKpa: number
  baroKpa: number
  boostKpa: number
  lambda: number
  afr: number

  ambientLux: number
  steeringDeg: number

  lights: boolean
  reverse: boolean

  // for averaging
  _avgWindowS: number
  _avgFuelL: number
  _avgDistKm: number
}

function createInitialState(): SimState {
  return {
    gear: 'P',
    shifter: 'P',
    driveGear: 1,
    driveMode: 'D',

    speedKph: 0,
    rpm: 800,

    coolantC: 55,
    oilC: 60,
    transmissionC: 45,
    iatC: 18,
    ambientC: 12,

    fuelPct: 53,
    rangeKm: 520,
    fuelRateLph: 0.8,
    consumptionLPer100Km: 0,
    consumptionAvgLPer100Km: 7.2,

    batteryV: 13.9,

    baroKpa: 101.3,
    mapKpa: 35,
    boostKpa: 0,
    lambda: 1.0,
    afr: 14.7,

    ambientLux: 220,
    steeringDeg: 0,

    lights: false,
    reverse: false,

    _avgWindowS: 0,
    _avgFuelL: 0,
    _avgDistKm: 0
  }
}

function stepThermals(s: SimState, dtS: number, load01: number) {
  const airflow01 = clamp(s.speedKph / 120, 0, 1)
  const coolantTarget = lerp(70, 102, load01) - lerp(0, 6, airflow01)
  const oilTarget = lerp(75, 112, load01) - lerp(0, 4, airflow01)
  const transTarget = lerp(55, 105, load01) - lerp(0, 5, airflow01)
  const iatTarget = s.ambientC + lerp(2, 25, load01) - lerp(0, 10, airflow01)

  const tau = 12
  const k = clamp(dtS / tau, 0, 1)

  s.coolantC = lerp(s.coolantC, coolantTarget, k)
  s.oilC = lerp(s.oilC, oilTarget, k * 0.85)
  s.transmissionC = lerp(s.transmissionC, transTarget, k * 0.9)
  s.iatC = lerp(s.iatC, iatTarget, k * 1.2)
}

function stepEngineSignals(s: SimState, dtS: number, throttle01: number) {
  const idle = 780

  // Target RPM based on shifter + 6-speed gear model in D
  let rpmTarget = idle

  if (s.shifter === 'D') {
    // base rpm from speed + current gear
    const base = rpmFromSpeedAndGear(s.speedKph, s.driveGear)

    // small “throttle load” effect so it wiggles under load
    const loadExtra = throttle01 * 220

    rpmTarget = clamp(base + loadExtra, idle, MAX_RPM)

    // auto upshift at ~5000 rpm
    if (s.driveGear < 6 && rpmTarget >= SHIFT_UP_RPM) {
      s.driveGear = clampGear(s.driveGear + 1)
    }
  } else if (s.shifter === 'R') {
    // Reverse: keep it low
    rpmTarget = clamp(idle + throttle01 * 900, idle, 2200)
  } else {
    // P / N
    rpmTarget = clamp(idle + throttle01 * 600, idle, 2000)
  }

  const tau = 0.25
  const k = clamp(dtS / tau, 0, 1)
  s.rpm = lerp(s.rpm, rpmTarget, k)

  // pressures:
  s.baroKpa = 101.3

  const mapNoBoost = lerp(34, 95, throttle01)
  const boostTarget = throttle01 > 0.6 ? lerp(0, 140, (throttle01 - 0.6) / 0.4) : 0
  s.boostKpa = lerp(s.boostKpa, boostTarget, clamp(dtS / 0.6, 0, 1))

  s.mapKpa = clamp(mapNoBoost + 0.35 * s.boostKpa, 20, 250)

  const lambdaTarget = s.boostKpa > 10 ? 0.86 : throttle01 > 0.8 ? 0.94 : 1.0
  s.lambda = lerp(s.lambda, lambdaTarget, clamp(dtS / 0.8, 0, 1))
  s.afr = 14.7 * s.lambda
}

function stepFuelAndRange(s: SimState, dtS: number, throttle01: number) {
  // approximate fuel rate:
  // idle 0.7-1.0 L/h; heavy throttle up to ~18 L/h
  const rateTarget = lerp(0.8, 18, throttle01)
  s.fuelRateLph = lerp(s.fuelRateLph, rateTarget, clamp(dtS / 1.2, 0, 1))

  // instant consumption:
  // if speed < 2 => show 0
  if (s.speedKph < 2) {
    s.consumptionLPer100Km = 0
  } else {
    // L/100km = (L/h) / (km/h) * 100
    s.consumptionLPer100Km = (s.fuelRateLph / s.speedKph) * 100
  }

  // average: integrate fuel + distance and compute average
  const distKm = (s.speedKph * dtS) / 3600
  const fuelL = (s.fuelRateLph * dtS) / 3600

  s._avgWindowS += dtS
  s._avgFuelL += fuelL
  s._avgDistKm += distKm

  // every ~20s, “publish” a refreshed average and slowly forget older history
  if (s._avgWindowS > 20) {
    const avg =
      s._avgDistKm > 0.001 ? (s._avgFuelL / s._avgDistKm) * 100 : s.consumptionAvgLPer100Km
    s.consumptionAvgLPer100Km = lerp(s.consumptionAvgLPer100Km, avg, 0.35)

    s._avgWindowS *= 0.6
    s._avgFuelL *= 0.6
    s._avgDistKm *= 0.6
  }

  // fuel level / range: very slow
  // burn fuel based on fuelL; assume 10L tank
  const tankL = 10
  const fuelPct = clamp(s.fuelPct - (fuelL / tankL) * 100, 0, 100)
  s.fuelPct = fuelPct

  // range based on avg consumption
  const remainingL = (fuelPct / 100) * tankL
  const cons = clamp(s.consumptionAvgLPer100Km, 3.5, 25)
  s.rangeKm = (remainingL / cons) * 100
}

function stepElectrical(s: SimState, dtS: number, load01: number) {
  // battery voltage: alternator when running; small sag if lots of load
  const base = 14.1
  const sag = s.lights ? 0.15 : 0.03
  const loadSag = lerp(0.02, 0.22, load01)
  const vTarget = base - sag - loadSag
  s.batteryV = lerp(s.batteryV, vTarget, clamp(dtS / 2.5, 0, 1))
}

function stepEnvironment(s: SimState, dtS: number, t: number) {
  // ambient lux: day/night-ish wave (for UI testing)
  const day01 = (Math.sin(t * 0.02) + 1) / 2
  const luxTarget = lerp(30, 1200, day01)
  s.ambientLux = lerp(s.ambientLux, luxTarget, clamp(dtS / 4, 0, 1))

  // lights on when it's “darker”
  s.lights = s.ambientLux < 120
}

// ─────────────────────────────────────────────────────────────────────────────
// Drive cycle (P -> N -> D, accelerate, cruise, stop, reverse etc.)
// ─────────────────────────────────────────────────────────────────────────────

async function driveCycle(socket: any) {
  const dt = 20 // 50 Hz
  const dtS = dt / 1000
  const s = createInitialState()
  const t0 = Date.now()

  const logEveryMs = 900
  let lastLog = 0

  while (true) {
    const t = (Date.now() - t0) / 1000

    // Phase helpers
    const inRange = (a: number, b: number) => t >= a && t < b
    const phaseT = (a: number, b: number) => clamp((t - a) / (b - a), 0, 1)

    // Defaults
    let throttle01 = 0
    let speedTarget = 0
    let steerTarget = 0
    let shifter: Shifter = s.shifter

    if (inRange(0, 2)) {
      shifter = 'P'
      speedTarget = 0
      throttle01 = 0.05
    } else if (inRange(2, 3)) {
      shifter = 'N'
      speedTarget = 0
      throttle01 = 0.03
    } else if (inRange(3, 4)) {
      shifter = 'D'
      speedTarget = 2
      throttle01 = 0.1
    } else if (inRange(4, 24)) {
      // Full pull: 0 -> 220 km/h (hits 1..6 and red area)
      shifter = 'D'
      const p = smoothstep(phaseT(4, 24))
      speedTarget = lerp(2, 220, p)

      // ramp throttle so rpm reaches shift point
      throttle01 = lerp(0.35, 0.95, p)

      steerTarget = Math.sin(t * 0.7) * 6
    } else if (inRange(24, 30)) {
      // Hold top speed briefly
      shifter = 'D'
      speedTarget = 220 + Math.sin(t * 0.4) * 1.5
      throttle01 = 0.28
      steerTarget = Math.sin(t * 0.5) * 4
    } else if (inRange(30, 40)) {
      // Brake to 0
      shifter = 'D'
      const p = smoothstep(phaseT(30, 40))
      speedTarget = lerp(220, 0, p)
      throttle01 = lerp(0.12, 0.03, p)
      steerTarget = 0
    } else if (inRange(40, 44)) {
      // Stop
      shifter = 'D'
      speedTarget = 0
      throttle01 = 0.03
      steerTarget = 0
    } else if (inRange(44, 54)) {
      // Reverse 10s (simulate reverse cam)
      shifter = 'R'
      const p = smoothstep(phaseT(44, 54))
      speedTarget = lerp(0, 8, p) // reverse speed
      throttle01 = lerp(0.08, 0.2, p)
      steerTarget = Math.sin(t * 0.9) * 10
    } else if (inRange(54, 58)) {
      // Stop after reverse
      shifter = 'R'
      speedTarget = 0
      throttle01 = 0.05
      steerTarget = 0
    } else {
      // Park + restart cycle
      shifter = 'P'
      speedTarget = 0
      throttle01 = 0.03
      steerTarget = 0
      break
    }

    // apply shifter
    s.shifter = shifter
    s.reverse = shifter === 'R'

    // reset drive gear when leaving D (so it starts from 1 next time)
    if (s.shifter !== 'D') s.driveGear = 1

    // IPC / UI display value:
    // - in D: show 1..6
    // - otherwise: show P/N/R
    s.gear =
      s.shifter === 'D' ? (`${s.driveMode}${s.driveGear}` as `${'D' | 'M'}${DriveGear}`) : s.shifter

    // speed dynamics (1st order)
    const tauSpeed = 0.9
    const kSpeed = clamp(dtS / tauSpeed, 0, 1)
    s.speedKph = lerp(s.speedKph, speedTarget, kSpeed)

    // steering smoothing
    s.steeringDeg = lerp(s.steeringDeg, steerTarget, clamp(dtS / 0.35, 0, 1))

    // derived load from throttle + speed
    const load01 = clamp(throttle01 * 0.85 + (s.speedKph / 200) * 0.25, 0, 1)

    // steps
    stepEnvironment(s, dtS, t)
    stepEngineSignals(s, dtS, throttle01)
    stepThermals(s, dtS, load01)
    stepFuelAndRange(s, dtS, throttle01)
    stepElectrical(s, dtS, load01)

    // publish
    push(socket, {
      speedKph: Math.round(s.speedKph),
      rpm: Math.round(s.rpm),
      gear: s.gear,
      steeringDeg: Math.round(s.steeringDeg),

      reverse: s.reverse,
      lights: s.lights,

      coolantC: Math.round(s.coolantC),
      oilC: Math.round(s.oilC),
      transmissionC: Math.round(s.transmissionC),
      iatC: Math.round(s.iatC),
      ambientC: Math.round(s.ambientC),

      batteryV: Number(s.batteryV.toFixed(2)),

      fuelPct: Number(s.fuelPct.toFixed(1)),
      rangeKm: Math.round(s.rangeKm),
      fuelRateLph: Number(s.fuelRateLph.toFixed(2)),
      consumptionLPer100Km: Number(s.consumptionLPer100Km.toFixed(2)),
      consumptionAvgLPer100Km: Number(s.consumptionAvgLPer100Km.toFixed(2)),

      mapKpa: Number(s.mapKpa.toFixed(1)),
      baroKpa: Number(s.baroKpa.toFixed(1)),
      boostKpa: Number(s.boostKpa.toFixed(1)),
      lambda: Number(s.lambda.toFixed(3)),
      afr: Number(s.afr.toFixed(2)),

      ambientLux: Math.round(s.ambientLux)
    })

    const now = Date.now()
    if (now - lastLog > logEveryMs) {
      lastLog = now
      console.log(
        `[telemetry-sim] cycle gear=${s.gear} v=${Math.round(s.speedKph)} rpm=${Math.round(s.rpm)} ` +
          `coolant=${Math.round(s.coolantC)} oil=${Math.round(s.oilC)} ` +
          `fuel=${s.fuelPct.toFixed(1)}% cons=${s.consumptionLPer100Km.toFixed(1)}L/100`
      )
    }

    await sleep(dt)
  }

  // restart cycle loop cleanly
  return driveCycle(socket)
}

// ─────────────────────────────────────────────────────────────────────────────
// Sweep
// ─────────────────────────────────────────────────────────────────────────────

async function sweep(socket: any) {
  const dt = 20
  const t0 = Date.now()

  const maxSpeed = 220

  while (true) {
    const t = (Date.now() - t0) / 1000
    const w = (Math.sin(t * 0.6) + 1) / 2 // 0..1

    const speedKph = Math.round(w * maxSpeed)
    const throttle01 = clamp(w * 0.9, 0, 1)

    // simple signals
    const rpm = Math.round(800 + throttle01 * 4200)
    const coolantC = Math.round(65 + w * 35)
    const oilC = Math.round(70 + w * 40)
    const transmissionC = Math.round(55 + w * 35)
    const iatC = Math.round(ambientCFromT(t) + w * 15)
    const ambientC = ambientCFromT(t)
    const fuelPct = 53
    const rangeKm = 520

    const boostKpa = throttle01 > 0.6 ? (throttle01 - 0.6) * 250 : 0
    const baroKpa = 101.3
    const mapKpa = clamp(34 + throttle01 * 70 + boostKpa * 0.35, 20, 250)
    const lambda = boostKpa > 10 ? 0.88 : 1.0
    const afr = 14.7 * lambda

    const fuelRateLph = 0.8 + throttle01 * 16
    const consumptionLPer100Km = speedKph < 2 ? 0 : (fuelRateLph / speedKph) * 100

    const ambientLux = Math.round(lerp(60, 1200, (Math.sin(t * 0.05) + 1) / 2))
    const lights = ambientLux < 140

    const payload = {
      speedKph,
      rpm,
      gear: speedKph < 2 ? 'N' : 'D',
      reverse: false,
      lights,

      coolantC,
      oilC,
      transmissionC,
      iatC,
      ambientC,

      fuelPct,
      rangeKm,
      fuelRateLph: Number(fuelRateLph.toFixed(2)),
      consumptionLPer100Km: Number(consumptionLPer100Km.toFixed(2)),
      consumptionAvgLPer100Km: 7.2,

      batteryV: Number((14.1 - throttle01 * 0.15).toFixed(2)),

      mapKpa: Number(mapKpa.toFixed(1)),
      baroKpa: Number(baroKpa.toFixed(1)),
      boostKpa: Number(boostKpa.toFixed(1)),
      lambda: Number(lambda.toFixed(3)),
      afr: Number(afr.toFixed(2)),

      ambientLux,
      steeringDeg: Math.round(Math.sin(t * 0.9) * 18)
    }

    push(socket, payload)

    if ((Date.now() - t0) % 700 < dt) {
      console.log(
        `[telemetry-sim] sweep v=${speedKph} rpm=${rpm} coolant=${coolantC} boost=${boostKpa.toFixed(0)}kPa`
      )
    }

    await sleep(dt)
  }
}

function ambientCFromT(t: number) {
  return Math.round(lerp(8, 18, (Math.sin(t * 0.03) + 1) / 2))
}

// ─────────────────────────────────────────────────────────────────────────────
// Set once (unchanged)
// ─────────────────────────────────────────────────────────────────────────────

async function setOnce(socket: any) {
  const speedKph = Number(process.argv[3] ?? 0)
  const rpm = Number(process.argv[4] ?? 0)
  const coolantC = Number(process.argv[5] ?? 0)

  const payload = {
    speedKph: clamp(speedKph, 0, 999),
    rpm: clamp(rpm, 0, 9999),
    coolantC: clamp(coolantC, -40, 140)
  }

  push(socket, payload)
  console.log('[telemetry-sim] push once:', payload)
  await sleep(200)
  process.exit(0)
}

// ─────────────────────────────────────────────────────────────────────────────

const socket = connect()

socket.on('connect', async () => {
  if (cmd === 'cycle') return driveCycle(socket)
  if (cmd === 'sweep') return sweep(socket)
  if (cmd === 'set') return setOnce(socket)

  console.log(`Usage:
  pnpm --dir scripts/tools telemetry:cycle
  pnpm --dir scripts/tools telemetry:sweep
  pnpm --dir scripts/tools telemetry:set -- <speedKph> <rpm> <coolantC>

Env:
  TELEMETRY_URL=http://127.0.0.1:4000
  TELEMETRY_SOURCE=sim
`)
  process.exit(1)
})
