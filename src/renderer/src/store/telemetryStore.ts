import { create } from 'zustand'

export type TelemetryPayload = {
  rpm?: number
  speedKmh?: number
  speedMph?: number
  engineTempC?: number
  rangeKm?: number
  instantConsumption?: number
  fuelLiters?: number
  can_ok?: number | boolean
  can_err?: string
  [key: string]: number | string | boolean | null | undefined
}

type Units = 'kmh' | 'mph'

type TelemetryState = {
  url: string
  connected: boolean
  connecting: boolean
  lastMessageAt: number | null
  lastError: string | null
  lastRawBytes: number | null
  payload: TelemetryPayload | null
  autoReconnect: boolean

  vehicleId: number
  supportedVehicles: Array<{ id: number; name: string }>
  tileOrder: string[]
  tileEnabled: Record<string, boolean>
  units: Units

  setVehicleId: (id: number) => void
  setTileEnabled: (key: string, enabled: boolean) => void
  setTileOrder: (order: string[]) => void
  setUnits: (units: Units) => void

  connect: () => void
  disconnect: () => void
  setUrl: (url: string) => void
  setAutoReconnect: (enabled: boolean) => void
  reset: () => void
}

const DEFAULT_URL = 'ws://localhost:8080/ws'
const RECONNECT_DELAYS_MS = [500, 1000, 2000, 5000, 8000]

const UI_STORAGE_KEY = 'telemetry.ui.v2'

type PersistedUI = {
  vehicleId: number
  tileOrder: string[]
  tileEnabled: Record<string, boolean>
  units: Units
}

const DEFAULT_TILES = ['speed', 'rpm', 'temp', 'consumption', 'range', 'fuel']

const DEFAULT_TILE_ENABLED: Record<string, boolean> = {
  speed: true,
  rpm: true,
  temp: true,
  consumption: true,
  range: true,
  fuel: true
}

const loadUI = (): PersistedUI | null => {
  try {
    const raw = localStorage.getItem(UI_STORAGE_KEY)
    if (!raw) return null
    const parsed = JSON.parse(raw) as PersistedUI
    if (!parsed || typeof parsed.vehicleId !== 'number') return null
    if (!Array.isArray(parsed.tileOrder)) return null
    if (!parsed.tileEnabled || typeof parsed.tileEnabled !== 'object') return null
    if (parsed.units !== 'kmh' && parsed.units !== 'mph') return null
    return parsed
  } catch {
    return null
  }
}

const saveUI = (ui: PersistedUI) => {
  try {
    localStorage.setItem(UI_STORAGE_KEY, JSON.stringify(ui))
  } catch {}
}

let socket: WebSocket | null = null
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let reconnectAttempt = 0

const clearReconnectTimer = () => {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer)
    reconnectTimer = null
  }
}

const scheduleReconnect = () => {
  const { autoReconnect, connecting, connected } = useTelemetryStore.getState()
  if (!autoReconnect || connecting || connected) return
  if (reconnectTimer) return

  const delay = RECONNECT_DELAYS_MS[Math.min(reconnectAttempt, RECONNECT_DELAYS_MS.length - 1)]
  reconnectAttempt += 1
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null
    useTelemetryStore.getState().connect()
  }, delay)
}

const CRC32_TABLE = (() => {
  const table = new Uint32Array(256)
  for (let i = 0; i < 256; i++) {
    let c = i
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    }
    table[i] = c >>> 0
  }
  return table
})()

function crc32(bytes: Uint8Array): number {
  let crc = 0xffffffff
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i]!
    crc = CRC32_TABLE[(crc ^ b) & 0xff]! ^ (crc >>> 8)
  }
  return (crc ^ 0xffffffff) >>> 0
}

function buildHandshakeFrame(modelId: number): Uint8Array {
  const cmd = 0x01
  const header = new Uint8Array(5)
  header[0] = cmd
  header[1] = modelId & 0xff
  header[2] = (modelId >>> 8) & 0xff
  header[3] = (modelId >>> 16) & 0xff
  header[4] = (modelId >>> 24) & 0xff

  const c = crc32(header)

  const frame = new Uint8Array(9)
  frame.set(header, 0)
  frame[5] = c & 0xff
  frame[6] = (c >>> 8) & 0xff
  frame[7] = (c >>> 16) & 0xff
  frame[8] = (c >>> 24) & 0xff
  return frame
}

const parseMessage = (data: MessageEvent['data']) => {
  if (typeof data === 'string') {
    try {
      return { payload: JSON.parse(data) as TelemetryPayload }
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Unknown parsing error'
      return { error: `Invalid JSON payload: ${message}` }
    }
  }

  if (data instanceof ArrayBuffer) {
    return { rawBytes: data.byteLength }
  }

  if (data instanceof Blob) {
    return { rawBytes: data.size }
  }

  return { error: 'Unsupported payload type' }
}

const setupSocket = (url: string) => {
  socket = new WebSocket(url)
  socket.binaryType = 'arraybuffer'

  socket.addEventListener('open', () => {
    reconnectAttempt = 0

    useTelemetryStore.setState({
      connected: true,
      connecting: false,
      lastError: null
    })

    try {
      socket?.send(buildHandshakeFrame(useTelemetryStore.getState().vehicleId))
    } catch (err) {
      useTelemetryStore.setState({
        lastError: err instanceof Error ? err.message : String(err)
      })
    }
  })

  socket.addEventListener('close', () => {
    useTelemetryStore.setState({ connected: false, connecting: false })
    scheduleReconnect()
  })

  socket.addEventListener('error', () => {
    useTelemetryStore.setState({ lastError: 'WebSocket error', connecting: false })
  })

  socket.addEventListener('message', (event) => {
    const result = parseMessage(event.data)
    const prev = useTelemetryStore.getState().payload

    useTelemetryStore.setState({
      lastMessageAt: Date.now(),
      payload: result.payload ?? prev,
      lastRawBytes: result.rawBytes ?? null,
      lastError: result.error ?? null
    })
  })
}

export const useTelemetryStore = create<TelemetryState>((set, get) => ({
  url: DEFAULT_URL,
  connected: false,
  connecting: false,
  lastMessageAt: null,
  lastError: null,
  lastRawBytes: null,
  payload: null,
  autoReconnect: true,

  ...(() => {
    const persisted = typeof window !== 'undefined' ? loadUI() : null
    return {
      vehicleId: persisted?.vehicleId ?? 0x00001234,
      supportedVehicles: [{ id: 0x00001234, name: 'BMW (default)' }],
      tileOrder: persisted?.tileOrder?.length ? persisted.tileOrder : DEFAULT_TILES,
      tileEnabled: { ...DEFAULT_TILE_ENABLED, ...(persisted?.tileEnabled ?? {}) },
      units: persisted?.units ?? 'kmh'
    }
  })(),

  setVehicleId: (id) => {
    set({ vehicleId: id })
    saveUI({
      vehicleId: id,
      tileOrder: get().tileOrder,
      tileEnabled: get().tileEnabled,
      units: get().units
    })

    clearReconnectTimer()

    if (socket) {
      try {
        socket.close()
      } catch {}
      socket = null
    }

    setTimeout(() => get().connect(), 0)
  },

  setTileEnabled: (key, enabled) => {
    const next = { ...get().tileEnabled, [key]: enabled }
    set({ tileEnabled: next })
    saveUI({
      vehicleId: get().vehicleId,
      tileOrder: get().tileOrder,
      tileEnabled: next,
      units: get().units
    })
  },

  setTileOrder: (order) => {
    set({ tileOrder: order })
    saveUI({
      vehicleId: get().vehicleId,
      tileOrder: order,
      tileEnabled: get().tileEnabled,
      units: get().units
    })
  },

  setUnits: (units) => {
    set({ units })
    saveUI({
      vehicleId: get().vehicleId,
      tileOrder: get().tileOrder,
      tileEnabled: get().tileEnabled,
      units
    })
  },

  connect: () => {
    const { url, connected, connecting } = get()
    if (connected || connecting) return

    clearReconnectTimer()
    set({ connecting: true })

    if (socket) {
      try {
        socket.close()
      } catch {}
      socket = null
    }

    setupSocket(url)
  },

  disconnect: () => {
    clearReconnectTimer()
    if (socket) {
      try {
        socket.close()
      } catch {}
      socket = null
    }
    set({ connected: false, connecting: false })
  },

  setUrl: (url) => set({ url }),

  setAutoReconnect: (enabled) => set({ autoReconnect: enabled }),

  reset: () =>
    set({
      lastMessageAt: null,
      lastError: null,
      lastRawBytes: null,
      payload: null
    })
}))
