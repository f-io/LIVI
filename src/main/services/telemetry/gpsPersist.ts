/**
 * GPS persistence — keeps the last known good GPS fix in the user config
 */

import { configEvents } from '@main/ipc/utils'
import type { Config, LastKnownGps } from '@shared/types'
import type { GpsPayload, TelemetryPayload } from '@shared/types/Telemetry'
import type { TelemetryStore } from './TelemetryStore'

const MIN_WRITE_INTERVAL_MS = 30_000

export type GpsPersistDeps = {
  store: TelemetryStore
  initialGps?: LastKnownGps | undefined
}

export type GpsPersistHandle = {
  off: () => void
}

export function attachGpsPersist({ store, initialGps }: GpsPersistDeps): GpsPersistHandle {
  if (isValidPersistedGps(initialGps)) {
    store.merge({
      gps: {
        lat: initialGps.lat,
        lng: initialGps.lng,
        alt: initialGps.alt,
        heading: initialGps.heading,
        fixTs: initialGps.ts
      }
    })
  }

  let lastWrittenAt = 0
  let pendingTimer: ReturnType<typeof setTimeout> | null = null
  let lastWrittenLat: number | undefined
  let lastWrittenLng: number | undefined

  const persist = (gps: GpsPayload): void => {
    const v = validateGps(gps.lat, gps.lng)
    if (!v) return
    if (v.lat === lastWrittenLat && v.lng === lastWrittenLng) return

    const payload: LastKnownGps = {
      lat: v.lat,
      lng: v.lng,
      ts: Date.now()
    }
    if (typeof gps.alt === 'number' && Number.isFinite(gps.alt)) payload.alt = gps.alt
    if (typeof gps.heading === 'number' && Number.isFinite(gps.heading))
      payload.heading = gps.heading

    lastWrittenLat = v.lat
    lastWrittenLng = v.lng
    lastWrittenAt = Date.now()

    try {
      configEvents.emit('requestSave', { lastKnownGps: payload } satisfies Partial<Config>)
    } catch (e) {
      console.warn('[gpsPersist] requestSave failed (ignored)', e)
    }
  }

  const onChange = (patch: TelemetryPayload, snap: TelemetryPayload): void => {
    if (!('gps' in patch)) return
    const gps = snap.gps
    if (!gps) return

    if (!validateGps(gps.lat, gps.lng)) return

    const now = Date.now()
    const elapsed = now - lastWrittenAt

    if (elapsed >= MIN_WRITE_INTERVAL_MS) {
      persist(gps)
      return
    }

    if (pendingTimer) return
    pendingTimer = setTimeout(() => {
      pendingTimer = null
      const latest = store.snapshot().gps
      if (latest && validateGps(latest.lat, latest.lng)) persist(latest)
    }, MIN_WRITE_INTERVAL_MS - elapsed)
  }

  store.on('change', onChange)

  return {
    off: (): void => {
      store.off('change', onChange)
      if (pendingTimer) {
        clearTimeout(pendingTimer)
        pendingTimer = null
      }
    }
  }
}

// ──────────────────────────────────────────────────────────────────────────
// Validation
// ──────────────────────────────────────────────────────────────────────────

function validateGps(lat: unknown, lng: unknown): { lat: number; lng: number } | null {
  if (typeof lat !== 'number' || typeof lng !== 'number') return null
  if (!Number.isFinite(lat) || !Number.isFinite(lng)) return null
  if (lat < -90 || lat > 90) return null
  if (lng < -180 || lng > 180) return null
  if (lat === 0 && lng === 0) return null
  return { lat, lng }
}

/** Same checks plus a present `ts` for the persisted form. */
function isValidPersistedGps(g: LastKnownGps | undefined): g is LastKnownGps {
  if (!g || typeof g !== 'object') return false
  if (!validateGps(g.lat, g.lng)) return false
  if (typeof g.ts !== 'number' || !Number.isFinite(g.ts)) return false
  return true
}
