import { registerIpcHandle } from '@main/ipc/register'
import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'
import { configEvents, saveSettings } from '@main/ipc/utils'
import { GhRelease, runtimeStateProps } from '@main/types'
import { currentKiosk } from '@main/window/utils'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { ExtraConfig } from '@shared/types'
import { app } from 'electron'

export function registerSettingsIpc(runtimeState: runtimeStateProps) {
  registerIpcHandle('settings:get-kiosk', () => currentKiosk(runtimeState.config))

  registerIpcHandle('getSettings', () => runtimeState.config)

  registerIpcHandle('save-settings', (_evt, settings: Partial<ExtraConfig>) => {
    saveSettings(runtimeState, settings)
    return true
  })

  configEvents.on('requestSave', (settings: Partial<ExtraConfig>) => {
    saveSettings(runtimeState, settings)
  })

  registerIpcHandle('settings:reset-dongle-icons', () => {
    const next: ExtraConfig = {
      ...runtimeState.config,
      dongleIcon120: ICON_120_B64,
      dongleIcon180: ICON_180_B64,
      dongleIcon256: ICON_256_B64
    }

    saveSettings(runtimeState, next)

    return {
      dongleIcon120: next.dongleIcon120,
      dongleIcon180: next.dongleIcon180,
      dongleIcon256: next.dongleIcon256
    }
  })

  registerIpcHandle('app:getVersion', () => app.getVersion())

  registerIpcHandle('app:getLatestRelease', async () => {
    try {
      const repo = process.env.UPDATE_REPO || 'f-io/LIVI'
      const feed = process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
      const res = await fetch(feed, { headers: { 'User-Agent': 'LIVI-updater' } })
      if (!res.ok) throw new Error(`feed ${res.status}`)
      const json = (await res.json()) as unknown as GhRelease
      const raw = (json.tag_name || json.name || '').toString()
      const version = raw.replace(/^v/i, '')
      const { url } = pickAssetForPlatform(json.assets || [])
      return { version, url }
    } catch (e) {
      console.warn('[update] getLatestRelease failed:', e)
      return { version: '', url: undefined }
    }
  })
}
