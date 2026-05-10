/**
 * LIVI Dash adapter — forwards telemetry to the renderer over IPC.
 */

import type { TelemetryPayload } from '@shared/types/Telemetry'
import type { WebContents } from 'electron'
import type { TelemetryStore } from '../TelemetryStore'

export type LiviDashAdapterDeps = {
  getWebContents: () => WebContents | WebContents[] | null
  store: TelemetryStore
}

export function attachLiviDashAdapter({ store, getWebContents }: LiviDashAdapterDeps): () => void {
  const onChange = (_patch: TelemetryPayload, snapshot: TelemetryPayload): void => {
    const wcs = getWebContents()
    if (!wcs) return
    const list = Array.isArray(wcs) ? wcs : [wcs]
    for (const wc of list) {
      if (!wc || wc.isDestroyed()) continue
      try {
        wc.send('telemetry:update', snapshot)
      } catch (e) {
        console.warn('[liviDashAdapter] webContents.send failed (ignored)', e)
      }
    }
  }

  store.on('change', onChange)
  return () => store.off('change', onChange)
}
