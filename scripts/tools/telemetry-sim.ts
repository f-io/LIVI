/**
 * LIVI Telemetry CLI — push test data into the running app.
 *
 * The full API surface lives in `src/main/shared/types/Telemetry.ts`.
 * This script is just a thin Socket.IO client that hits `ws://127.0.0.1:4000` event `telemetry:push`.
 *
 * USAGE
 * ─────
 *
 *   pnpm --dir scripts/tools telemetry:set <field>=<value> [<field>=<value> …]
 *   pnpm --dir scripts/tools telemetry:demo
 *
 * SEND A SINGLE FIELD
 *
 *   telemetry:set speedKph=73
 *   telemetry:set nightMode=true
 *   telemetry:set turn=left
 *
 *
 * SEND A BLOCK (multiple fields in one push, merged on the LIVI side)
 *
 *   telemetry:set speedKph=73 rpm=2100 gear=D lights=true
 *   telemetry:set fuelPct=4 rangeKm=38
 *
 *
 * SEND A NESTED BLOCK (sub-objects use dot-notation; gps, can are merged)
 *
 *   telemetry:set gps.lat=53.5912 gps.lng=10.015 gps.heading=90
 *
 *
 * REPEAT THE SAME PUSH ON A TIMER (e.g. for live streaming)
 *
 *   telemetry:set _repeatMs=1000 speedKph=90 rpm=2500
 *
 *
 * ALL-AT-ONCE DEMO  (one push that fills every meaningful field)
 *
 *   telemetry:demo
 *
 *
 * ENV
 *
 *   TELEMETRY_URL=http://127.0.0.1:4000
 *   TELEMETRY_SOURCE=sim
 */

import { io, type Socket } from 'socket.io-client'

const URL = process.env.TELEMETRY_URL ?? 'http://127.0.0.1:4000'
const SOURCE = process.env.TELEMETRY_SOURCE ?? 'sim'

const cmd = process.argv[2] ?? 'help'

// ──────────────────────────────────────────────────────────────────────────
// Connect
// ──────────────────────────────────────────────────────────────────────────

function connect(): Socket {
  const socket: Socket = io(URL, { transports: ['websocket'] })

  socket.on('connect', () => {
    console.log(`[telemetry] connected ${socket.id} → ${URL} (source=${SOURCE})`)
  })

  socket.on('connect_error', (e) => {
    console.error('[telemetry] connect_error:', (e as { message?: string })?.message ?? e)
  })

  return socket
}

function push(socket: Socket, payload: Record<string, unknown>): void {
  socket.emit('telemetry:push', { ts: Date.now(), source: SOURCE, ...payload })
}

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms))

// ──────────────────────────────────────────────────────────────────────────
// `set` — push one or more `key=value` pairs once (or repeated)
// ──────────────────────────────────────────────────────────────────────────

/** Coerce `key=value` into a typed pair (numbers, bools, null, string). */
function parseKv(raw: string): [string, unknown] | null {
  const eq = raw.indexOf('=')
  if (eq <= 0) return null
  const key = raw.slice(0, eq).trim()
  const value = raw.slice(eq + 1).trim()
  if (!key) return null
  if (value === 'true') return [key, true]
  if (value === 'false') return [key, false]
  if (value === 'null') return [key, null]
  if (value === '') return [key, '']
  const n = Number(value)
  if (!Number.isNaN(n) && Number.isFinite(n)) return [key, n]
  return [key, value]
}

/** Inflate dotted keys (`gps.lat`) into nested objects. */
function inflate(flat: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [k, v] of Object.entries(flat)) {
    if (!k.includes('.')) {
      out[k] = v
      continue
    }
    const parts = k.split('.')
    let cur: Record<string, unknown> = out
    for (let i = 0; i < parts.length - 1; i++) {
      const seg = parts[i]!
      if (typeof cur[seg] !== 'object' || cur[seg] === null) cur[seg] = {}
      cur = cur[seg] as Record<string, unknown>
    }
    cur[parts[parts.length - 1]!] = v
  }
  return out
}

async function setOnce(socket: Socket): Promise<void> {
  const args = process.argv.slice(3)
  const flat: Record<string, unknown> = {}
  let repeatMs = 0

  for (const raw of args) {
    const kv = parseKv(raw)
    if (!kv) {
      console.error(`[telemetry] ignoring malformed arg: ${raw}`)
      continue
    }
    if (kv[0] === '_repeatMs' && typeof kv[1] === 'number') {
      repeatMs = kv[1]
      continue
    }
    flat[kv[0]] = kv[1]
  }

  if (Object.keys(flat).length === 0) {
    console.error('[telemetry] no fields given. Examples:')
    console.error('  telemetry:set fuelPct=4 rangeKm=38')
    console.error('  telemetry:set speedKph=50 rpm=1500 gear=4 lights=true nightMode=true')
    console.error('  telemetry:set gps.lat=53.5912 gps.lng=10.015 gps.heading=90')
    process.exit(1)
  }

  const payload = inflate(flat)
  push(socket, payload)
  console.log('[telemetry] push:', JSON.stringify(payload))

  if (repeatMs > 0) {
    setInterval(() => push(socket, payload), repeatMs)
    console.log(`[telemetry] repeating every ${repeatMs} ms — Ctrl+C to stop`)
    return
  }

  await sleep(200)
  process.exit(0)
}

// ──────────────────────────────────────────────────────────────────────────
// `demo` — one push filling every meaningful field with realistic values
// ──────────────────────────────────────────────────────────────────────────

async function demo(socket: Socket): Promise<void> {
  const payload = {
    // Motion / cluster basics
    speedKph: 50,
    rpm: 1500,
    gear: 4,
    steeringDeg: 0,

    // Driver-facing booleans
    reverse: false,
    lights: true,
    highBeam: false,
    hazards: false,
    turn: 'none',
    parkingBrake: false,

    // Temperatures (°C)
    coolantC: 90,
    oilC: 95,
    transmissionC: 80,
    iatC: 28,
    ambientC: 20,

    // Electrical
    batteryV: 14.1,

    // Fuel
    fuelPct: 4,
    rangeKm: 38,
    fuelRateLph: 6.4,
    consumptionLPer100Km: 12.8,
    consumptionAvgLPer100Km: 7.6,

    // Engine air / boost / fueling
    mapKpa: 55,
    baroKpa: 101.3,
    boostKpa: 0,
    lambda: 1.0,
    afr: 14.7,

    // Distance / driving status
    odometerKm: 87432.5,
    odometerTripKm: 142.7,
    drivingStatus: 0, // unrestricted

    // Environment
    ambientLux: 80, // dusk

    // External UI override
    nightMode: true,

    // GPS
    gps: {
      lat: 53.55773224530399,
      lng: 9.997866754244244,
      alt: 8,
      heading: 90,
      speedMs: 13.89,
      accuracyM: 4,
      satellites: 11
    }
  }

  push(socket, payload)
  console.log('[telemetry] demo push:')
  console.log(JSON.stringify(payload, null, 2))

  await sleep(200)
  process.exit(0)
}

// ──────────────────────────────────────────────────────────────────────────
// Help
// ──────────────────────────────────────────────────────────────────────────

function help(): never {
  console.log(`LIVI Telemetry CLI

  pnpm --dir scripts/tools telemetry:set <field>=<value> [<field>=<value> …]
  pnpm --dir scripts/tools telemetry:demo

Examples
  telemetry:set speedKph=73                       # one field
  telemetry:set fuelPct=4 rangeKm=38              # block (low-fuel warning)
  telemetry:set gps.lat=53.5912 gps.lng=10.015    # nested block (gps)
  telemetry:set _repeatMs=1000 speedKph=90        # repeat every 1 s
  telemetry:demo                                  # one realistic all-fields push

Reference
  src/main/shared/types/Telemetry.ts              # full field list + routing

Env
  TELEMETRY_URL=${URL}
  TELEMETRY_SOURCE=${SOURCE}
`)
  process.exit(cmd === 'help' ? 0 : 1)
}

// ──────────────────────────────────────────────────────────────────────────
// Dispatch
// ──────────────────────────────────────────────────────────────────────────

const socket = connect()

socket.on('connect', async () => {
  if (cmd === 'set') return setOnce(socket)
  if (cmd === 'demo') return demo(socket)
  help()
})
