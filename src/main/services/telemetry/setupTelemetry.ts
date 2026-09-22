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
 *   │ liviDashAdapt    │   aaAdapter   │
 *   │ (IPC → Renderer) │  (AaDriver)   │
 *   └──────────────────┴───────────────┴───────────────┘
 *
 */

import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { configEvents } from '@main/ipc/utils'
import type { ProjectionService } from '@main/services/projection/services/ProjectionService'
import { getAllRendererWebContents } from '@main/window/broadcast'
import type { Config } from '@shared/types'
import type { TelemetryPayload } from '@shared/types/Telemetry'
import { ipcMain } from 'electron'
import { attachAaAdapter } from './adapters/aaAdapter'
import { attachBlinkerSound } from './adapters/blinkerSoundAdapter'
import { attachCpAdapter } from './adapters/cpAdapter'
import { attachLiviDashAdapter } from './adapters/liviDashAdapter'
import { attachGnss } from './gnss/attachGnss'
import { attachGpsPersist } from './gpsPersist'
import type { TelemetryStore } from './TelemetryStore'
import { attachVolumePersist } from './volumePersist'

export type SetupTelemetryDeps = {
  store: TelemetryStore
  projectionService?: ProjectionService
  initialConfig?: Config
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

  const volumePersist = attachVolumePersist({
    store,
    initialVolume: initialConfig?.huVolume
  })

  const gnss = attachGnss({ store, initialConfig })

  let lastAppearanceMode: string | undefined = initialConfig?.appearanceMode
  const onConfigChanged = (merged: Config): void => {
    if (merged.appearanceMode !== lastAppearanceMode) {
      lastAppearanceMode = merged.appearanceMode
      applyAppearanceMode(store, merged.appearanceMode)
    }
    gnss.applyConfig(merged)
  }
  configEvents.on('changed', onConfigChanged)

  // ── Adapters ────────────────────────────────────────────────────────────

  const offDash = attachLiviDashAdapter({
    store,
    getWebContents: () => getAllRendererWebContents()
  })

  let offAa: (() => void) | null = null
  let offCp: (() => void) | null = null
  let offPlugHook: (() => void) | null = null
  let offBlinker: (() => void) | null = null
  if (projectionService) {
    offBlinker = attachBlinkerSound({
      store,
      setActive: (active) => projectionService.setBlinkerSoundActive(active)
    })

    const aa = attachAaAdapter({
      store,
      getAaDriver: () => projectionService.getAaDriver()
    })
    const cp = attachCpAdapter({
      store,
      getCpDriver: () => projectionService.getCpDriver()
    })
    offAa = aa.off
    offCp = cp.off

    offPlugHook = projectionService.addPluggedHook(() => {
      try {
        aa.hydrate()
      } catch (e) {
        console.warn('[setupTelemetry] aa.hydrate threw (ignored)', e)
      }
      try {
        cp.hydrate()
      } catch (e) {
        console.warn('[setupTelemetry] cp.hydrate threw (ignored)', e)
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
      volumePersist.off()
      gnss.dispose()
      offDash()
      offAa?.()
      offCp?.()
      offPlugHook?.()
      offBlinker?.()
    }
  }
}

function applyAppearanceMode(store: TelemetryStore, mode: string | undefined): void {
  if (mode === 'night') store.merge({ nightMode: true })
  else if (mode === 'day') store.merge({ nightMode: false })
}
