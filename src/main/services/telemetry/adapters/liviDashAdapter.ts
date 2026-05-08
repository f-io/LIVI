/**
 * LIVI Dash adapter — forwards telemetry to the renderer over IPC.
 */

import type { TelemetryPayload } from '@shared/types/Telemetry'
import type { WebContents } from 'electron'
import type { TelemetryStore } from '../TelemetryStore'

export type LiviDashAdapterDeps = {
  getWebContents: () => WebContents | null
  store: TelemetryStore
}

export function attachLiviDashAdapter({ store, getWebContents }: LiviDashAdapterDeps): () => void {
  const onChange = (_patch: TelemetryPayload, snapshot: TelemetryPayload): void => {
    const wc = getWebContents()
    if (!wc || wc.isDestroyed()) return
    try {
      wc.send('telemetry:update', snapshot)
    } catch (e) {
      console.warn('[liviDashAdapter] webContents.send failed (ignored)', e)
    }
  }

  store.on('change', onChange)
  return () => store.off('change', onChange)
}
