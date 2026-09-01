/**
 * LIVI Telemetry CLI — push test data into the running app, or simulate a remote
 * telemetry source for LIVI's "Client" (Telemetry (IPC)) mode.
 *
 * The full API surface lives in `src/main/shared/types/Telemetry.ts`.
 *
 * This script can play either side of LIVI's telemetry socket:
 *
 *   - `set` / `demo` — a Socket.IO CLIENT that connects to a LIVI running in
 *     "Host" mode and pushes data in via `telemetry:push`. (LIVI owns the socket;
 *     this script is one of potentially many things feeding it.)
 *
 *   - `serve` — a Socket.IO SERVER that a LIVI running in "Client" mode can point
 *     at (Settings → General → Telemetry (IPC) → Client → IP/Port) and receive
 *     `telemetry:update` broadcasts from, exactly like it would from a remote LIVI
 *     in Host mode. Useful for exercising client mode without a second real LIVI.
 *
 * USAGE
 * ─────
 *
 *   pnpm --dir scripts/tools telemetry:set <field>=<value> [<field>=<value> …]
 *   pnpm --dir scripts/tools telemetry:demo
 *   pnpm --dir scripts/tools telemetry:serve [<field>=<value> …]
 *
 * SEND A SINGLE FIELD (into a Host-mode LIVI)
 *
 *   telemetry:set speedKph=73
 *   telemetry:set nightMode=true
 *   telemetry:set turn=left
 *
 *
 * NAVIGATE THE UI (router path; a path matching no route is discarded)
 *
 *   telemetry:set path=/media
 *   telemetry:set path=/settings/devices
 *   telemetry:set path=/
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
 * SIMULATE A REMOTE HOST  (for LIVI's "Client" telemetry mode)
 *
 *   telemetry:serve                                 # serves the demo payload once per connection
 *   telemetry:serve speedKph=73 rpm=2100             # serves a custom payload instead
 *   telemetry:serve _repeatMs=1000 speedKph=90       # keeps broadcasting on a timer
 *
 *   Point LIVI at this process: Settings → General → Telemetry (IPC) → Client,
 *   IP = this machine's address, Port = TELEMETRY_URL's port (4000 by default).
 *
 *
 * ENV
 *
 *   TELEMETRY_URL=http://127.0.0.1:4000   (for `set`/`demo`: where to connect to.
 *                                           for `serve`: which port to listen on.)
 *   TELEMETRY_SOURCE=sim
 */

import { createServer } from 'http'
import { Server as IOServer } from 'socket.io'
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

/** Realistic values for every meaningful field — shared by `demo` (push once into a
 *  Host-mode LIVI) and `serve` with no fields given (broadcast to a Client-mode LIVI). */
function buildDemoPayload(): Record<string, unknown> {
  return {
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
}

async function demo(socket: Socket): Promise<void> {
  const payload = buildDemoPayload()

  push(socket, payload)
  console.log('[telemetry] demo push:')
  console.log(JSON.stringify(payload, null, 2))

  await sleep(200)
  process.exit(0)
}

// ──────────────────────────────────────────────────────────────────────────
// `serve` — host a Socket.IO server that a Client-mode LIVI can connect to
// ──────────────────────────────────────────────────────────────────────────

/** Same `key=value [...]` parsing as `set`, but for building the payload `serve`
 *  broadcasts rather than pushing it anywhere. Falls back to the demo payload when
 *  no fields are given, so `telemetry:serve` alone is a useful smoke test on its own. */
function parseServeArgs(): { payload: Record<string, unknown>; repeatMs: number } {
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

  const payload = Object.keys(flat).length > 0 ? inflate(flat) : buildDemoPayload()
  return { payload, repeatMs }
}

async function serve(): Promise<void> {
  const port = (() => {
    try {
      const parsed = new globalThis.URL(URL).port
      return parsed ? Number(parsed) : 4000
    } catch {
      return 4000
    }
  })()

  const { payload, repeatMs } = parseServeArgs()
  const envelope = (): Record<string, unknown> => ({ ts: Date.now(), source: SOURCE, ...payload })

  const httpServer = createServer()
  const ioServer = new IOServer(httpServer, { cors: { origin: '*' } })

  ioServer.on('connection', (socket) => {
    console.log(`[telemetry] LIVI client connected (${socket.id}) — sending telemetry:update`)
    socket.emit('telemetry:update', envelope())
    socket.on('disconnect', (reason) => {
      console.log(`[telemetry] LIVI client disconnected: ${reason}`)
    })
  })

  httpServer.on('error', (err: NodeJS.ErrnoException) => {
    console.error(`[telemetry] server error on port ${port}:`, err.message)
  })

  httpServer.listen(port, () => {
    console.log(`[telemetry] serving telemetry:update on :${port} (source=${SOURCE})`)
    console.log(
      '[telemetry] point a LIVI "Client" telemetry setting at this machine\'s IP and this port'
    )
    console.log('[telemetry] payload:', JSON.stringify(payload))
  })

  if (repeatMs > 0) {
    setInterval(() => ioServer.emit('telemetry:update', envelope()), repeatMs)
    console.log(`[telemetry] broadcasting every ${repeatMs} ms — Ctrl+C to stop`)
  } else {
    console.log('[telemetry] broadcasting the same payload to each new connection — Ctrl+C to stop')
  }
}

// ──────────────────────────────────────────────────────────────────────────
// Help
// ──────────────────────────────────────────────────────────────────────────

function help(): never {
  console.log(`LIVI Telemetry CLI

  pnpm --dir scripts/tools telemetry:set <field>=<value> [<field>=<value> …]
  pnpm --dir scripts/tools telemetry:demo
  pnpm --dir scripts/tools telemetry:serve [<field>=<value> …]

Examples
  telemetry:set speedKph=73                       # one field → Host-mode LIVI
  telemetry:set fuelPct=4 rangeKm=38              # block (low-fuel warning)
  telemetry:set gps.lat=53.5912 gps.lng=10.015    # nested block (gps)
  telemetry:set _repeatMs=1000 speedKph=90        # repeat every 1 s
  telemetry:demo                                  # one realistic all-fields push
  telemetry:serve                                 # simulate a remote host for Client-mode LIVI
  telemetry:serve _repeatMs=500 speedKph=90       # ...broadcasting on a timer

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

if (cmd === 'serve') {
  void serve()
} else {
  const socket = connect()

  socket.on('connect', async () => {
    if (cmd === 'set') return setOnce(socket)
    if (cmd === 'demo') return demo(socket)
    help()
  })
}
