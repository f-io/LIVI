import { app, ipcMain } from 'electron'
import { ExtraConfig } from '@main/Globals'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@main/services/carplay/assets/carIcons'
import { currentKiosk } from '@main/window/utils'
import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'
import { GhRelease, runtimeStateProps } from '@main/types'
import { saveSettings } from '@main/ipc/utils'

export function registerSettingsIpc(runtimeState: runtimeStateProps) {
  ipcMain.handle('settings:get-kiosk', () => currentKiosk(runtimeState.config))

  ipcMain.handle('getSettings', () => runtimeState.config)

  ipcMain.handle('save-settings', (_evt, settings: Partial<ExtraConfig>) => {
    saveSettings(runtimeState, settings)
    return true
  })

  ipcMain.handle('settings:reset-dongle-icons', () => {
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

  ipcMain.handle('app:getVersion', () => app.getVersion())

  ipcMain.handle('app:getLatestRelease', async () => {
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
