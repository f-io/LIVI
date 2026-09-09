import { listHostOutputModes } from '@main/app/hostOutput'
import {
  listBtAdapters,
  listWifiChannels,
  listWifiCountryCodes,
  listWifiInterfaces
} from '@main/app/wifiOptions'
import { registerIpcHandle } from '@main/ipc/register'
import { releaseFeedUrl, runNumberFromTitle } from '@main/ipc/update/feed'
import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'
import { configEvents, saveSettings } from '@main/ipc/utils'
import { DONGLE_LINK, dongleApPresent } from '@main/services/link/dongleAp'
import { GhRelease, runtimeStateProps } from '@main/types'
import { currentKiosk } from '@main/window/utils'
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

  registerIpcHandle('app:getVersion', () => app.getVersion())

  registerIpcHandle('app:listDisplayModes', () => listHostOutputModes())

  registerIpcHandle('app:listWifiChannels', () => listWifiChannels(runtimeState.config.wifiType))

  registerIpcHandle('app:listWifiCountryCodes', () => listWifiCountryCodes())

  // The dongle's radios are none of this host's, so they are offered next to them rather than found.
  registerIpcHandle('app:listWifiInterfaces', async () => {
    const local = listWifiInterfaces()
    const all = (await dongleApPresent()) ? [...local, DONGLE_LINK] : local
    console.log(`[settings] wifi interfaces: ${all.join(', ') || 'none'}`)
    return all
  })

  registerIpcHandle('app:listBtAdapters', async () => {
    const local = listBtAdapters()
    const all = (await dongleApPresent()) ? [...local, DONGLE_LINK] : local
    console.log(`[settings] bluetooth adapters: ${all.join(', ') || 'none'}`)
    return all
  })

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
