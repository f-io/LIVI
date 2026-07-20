/**
 * Head-unit volume ingestion — a telemetry `volume` value becomes `huVolume` in the config.
 *
 * This is the way in for anything outside LIVI that owns a volume control: steering-wheel
 * buttons, a radio on the serial bus, a dash app. From there `huVolume` drives the amplifier
 * and, when the link is enabled, the system mixer, so all sides show the same number.
 */

import { configEvents } from '@main/ipc/utils'
import type { Config } from '@shared/types'
import type { TelemetryPayload } from '@shared/types/Telemetry'
import type { TelemetryStore } from './TelemetryStore'

/** Ignore changes below this, so rounding noise does not rewrite the config. */
const MIN_DELTA = 0.005

export type VolumePersistDeps = {
  store: TelemetryStore
  initialVolume?: number | undefined
}

export type VolumePersistHandle = {
  off: () => void
}

export function attachVolumePersist({
  store,
  initialVolume
}: VolumePersistDeps): VolumePersistHandle {
  if (typeof initialVolume === 'number' && Number.isFinite(initialVolume)) {
    store.merge({ volume: clamp(initialVolume) })
  }

  let lastWritten = typeof initialVolume === 'number' ? clamp(initialVolume) : undefined

  const onChange = (_snap: TelemetryPayload, patch: TelemetryPayload): void => {
    if (!('volume' in patch)) return
    const raw = patch.volume
    if (typeof raw !== 'number' || !Number.isFinite(raw)) return
    const level = clamp(raw)
    if (lastWritten !== undefined && Math.abs(level - lastWritten) < MIN_DELTA) return
    lastWritten = level
    console.log(`[volume] head unit set to ${Math.round(level * 100)} % from telemetry`)
    try {
      configEvents.emit('requestSave', { huVolume: level } satisfies Partial<Config>)
    } catch (e) {
      console.warn('[volume] requestSave failed (ignored)', e)
    }
  }

  store.on('change', onChange)
  return {
    off: (): void => {
      store.off('change', onChange)
    }
  }
}

function clamp(v: number): number {
  return Math.min(1, Math.max(0, v))
}
