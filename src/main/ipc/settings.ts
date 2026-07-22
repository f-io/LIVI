import { registerIpcHandle } from '@main/ipc/register'
import { releaseFeedUrl, runNumberFromTitle } from '@main/ipc/update/feed'
import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'
import { configEvents, saveSettings } from '@main/ipc/utils'
import { GhRelease, runtimeStateProps } from '@main/types'
import { currentKiosk } from '@main/window/utils'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@shared/assets/carIcons'
import type { Config } from '@shared/types'
import { app } from 'electron'

export function registerSettingsIpc(runtimeState: runtimeStateProps) {
  registerIpcHandle('settings:get-kiosk', () => currentKiosk(runtimeState.config))

  registerIpcHandle('getSettings', () => runtimeState.config)

  registerIpcHandle('save-settings', (_evt, settings: Partial<Config>) => {
    saveSettings(runtimeState, settings)
    return true
  })

  configEvents.on('requestSave', (settings: Partial<Config>) => {
    saveSettings(runtimeState, settings)
  })

  registerIpcHandle('settings:reset-dongle-icons', () => {
    saveSettings(runtimeState, {
      dongleIcon120: '',
      dongleIcon180: '',
      dongleIcon256: ''
    })
    return {
      dongleIcon120: ICON_120_B64,
      dongleIcon180: ICON_180_B64,
      dongleIcon256: ICON_256_B64
    }
  })

  registerIpcHandle('app:getVersion', () => app.getVersion())

  registerIpcHandle('app:getLatestRelease', async () => {
    const nightly = runtimeState.config.updateNightly === true
    try {
      const res = await fetch(releaseFeedUrl(nightly), {
        headers: { 'User-Agent': 'LIVI-updater' }
      })
      if (!res.ok) throw new Error(`feed ${res.status}`)
      const json = (await res.json()) as unknown as GhRelease
      const raw = (json.tag_name || json.name || '').toString()
      const version = raw.replace(/^v/i, '')
      const { url } = pickAssetForPlatform(json.assets || [])
      const commit = (json.target_commitish || '').toString()
      const run = runNumberFromTitle(json.name)
      return { version, url, commit, run }
    } catch (e) {
      console.warn(`[update] getLatestRelease (${nightly ? 'nightly' : 'release'}) failed:`, e)
      return { version: '', url: undefined, commit: '', run: '' }
    }
  })
}
