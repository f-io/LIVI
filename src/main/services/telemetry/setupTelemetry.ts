/**
 * Telemetry entrypoint — owns the store, attaches every adapter.
 *
 *   ingestion (Socket.IO + IPC)
 *           │
 *           ▼
 *      TelemetryStore.merge(patch)
 *           │
 *           │  on 'change'
 *           ▼
 *   ┌──────────────────┬───────────────┬───────────────┐
 *   │ liviDashAdapt    │   aaAdapter   │ dongleAdapter │
 *   │ (IPC → Renderer) │  (AaDriver)   │ (DongleDriver)│
 *   └──────────────────┴───────────────┴───────────────┘
 *
 */

import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'
import type { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { getAllRendererWebContents } from '@main/window/broadcast'
import type { ExtraConfig } from '@shared/types'
import type { TelemetryPayload } from '@shared/types/Telemetry'
import { ipcMain } from 'electron'
import { attachAaAdapter } from './adapters/aaAdapter'
import { attachDongleAdapter } from './adapters/dongleAdapter'
import { attachLiviDashAdapter } from './adapters/liviDashAdapter'
import { attachGpsPersist } from './gpsPersist'
import type { TelemetryStore } from './TelemetryStore'

export type SetupTelemetryDeps = {
  store: TelemetryStore
  projectionService?: ProjectionService
  initialConfig?: ExtraConfig
}

export type TelemetryHandle = {
  store: TelemetryStore
  dispose: () => void
}

export function setupTelemetry({
  store,
  projectionService,
  initialConfig
}: SetupTelemetryDeps): TelemetryHandle {
  // Renderer-side IPC ingestion (kept symmetrical with Socket.IO).
  registerIpcOn<[TelemetryPayload | undefined]>('telemetry:push', (_evt, payload) => {
    store.merge(payload)
  })

  // Snapshot fetch — used by dashes on mount to hydrate
  registerIpcHandle('telemetry:snapshot', (): TelemetryPayload => store.snapshot())

  // ── Initial seed: appearanceMode + persisted GPS ────────────────────────

  applyAppearanceMode(store, initialConfig?.appearanceMode)

  const gpsPersist = attachGpsPersist({
    store,
    initialGps: initialConfig?.lastKnownGps
  })

  let lastAppearanceMode: string | undefined = initialConfig?.appearanceMode
  const onConfigChanged = (merged: ExtraConfig): void => {
    if (merged.appearanceMode !== lastAppearanceMode) {
      lastAppearanceMode = merged.appearanceMode
      applyAppearanceMode(store, merged.appearanceMode)
    }
  }
  configEvents.on('changed', onConfigChanged)

  // ── Adapters ────────────────────────────────────────────────────────────

  const offDash = attachLiviDashAdapter({
    store,
    getWebContents: () => getAllRendererWebContents()
  })

  let offAa: (() => void) | null = null
  let offDongle: (() => void) | null = null
  let offPlugHook: (() => void) | null = null
  if (projectionService) {
    const aa = attachAaAdapter({
      store,
      getAaDriver: () => projectionService.getAaDriver()
    })
    const dongle = attachDongleAdapter({
      store,
      getDongleDriver: () => projectionService.getDongleDriver()
    })
    offAa = aa.off
    offDongle = dongle.off

    offPlugHook = projectionService.addPluggedHook(() => {
      try {
        aa.hydrate()
      } catch (e) {
        console.warn('[setupTelemetry] aa.hydrate threw (ignored)', e)
      }
      try {
        dongle.hydrate()
      } catch (e) {
        console.warn('[setupTelemetry] dongle.hydrate threw (ignored)', e)
      }
    })
  }

  return {
    store,
    dispose: (): void => {
      ipcMain.removeAllListeners('telemetry:push')
      ipcMain.removeHandler('telemetry:snapshot')
      configEvents.off('changed', onConfigChanged)
      gpsPersist.off()
      offDash()
      offAa?.()
      offDongle?.()
      offPlugHook?.()
    }
  }
}

function applyAppearanceMode(store: TelemetryStore, mode: string | undefined): void {
  if (mode === 'night') store.merge({ nightMode: true })
  else if (mode === 'day') store.merge({ nightMode: false })
}
