/**
 * Blinker-sound adapter — drives the turn-signal relay click straight from the telemetry store,
 * so it sounds whenever the blinker is active regardless of which page/window is on screen.
 */

import type { TelemetryPayload } from '@shared/types/Telemetry'
import type { TelemetryStore } from '../TelemetryStore'

export type BlinkerSoundAdapterDeps = {
  store: TelemetryStore
  setActive: (active: boolean) => void
}

export function attachBlinkerSound({ store, setActive }: BlinkerSoundAdapterDeps): () => void {
  let last = false

  const onChange = (_patch: TelemetryPayload, snapshot: TelemetryPayload): void => {
    const active =
      snapshot.turn === 'left' || snapshot.turn === 'right' || snapshot.hazards === true
    if (active === last) return
    last = active
    try {
      setActive(active)
    } catch (e) {
      console.warn('[blinkerSoundAdapter] setActive failed (ignored)', e)
    }
  }

  store.on('change', onChange)
  return () => {
    store.off('change', onChange)
    if (last) {
      try {
        setActive(false)
      } catch {
        // ignore
      }
    }
  }
}
