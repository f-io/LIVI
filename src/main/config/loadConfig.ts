import { hostname } from 'node:os'
import type { Config } from '@shared/types'
import { DEFAULT_CONFIG } from '@shared/types'
import { CAR_NAME_MAX, WIFI_PASSWORD_MAX, WIFI_PASSWORD_MIN } from '@shared/types/Config'
import { isPagePath } from '@shared/types/Pages'
import { existsSync, readFileSync } from 'fs'
import { sysfsPanelGeometry } from '../services/video/panelEdid'
import { CONFIG_BACKUP_PATH, CONFIG_PATH } from './paths'
import { validate } from './validateConfig'
import { configForFile, writeConfig } from './writeConfig'

/** carName names the Wi-Fi AP, the Bluetooth device and the head unit, so two
 *  cars on the stock name would collide. The host already carries a name the
 *  owner picked, so a fresh install takes that one. */
function carNameFromHost(): string {
  const name = hostname().split('.')[0].trim()
  if (!name || name.toLowerCase() === 'localhost') return DEFAULT_CONFIG.carName
  return name.slice(0, CAR_NAME_MAX)
}

export function loadConfig(): Config {
  let fileConfig: Partial<Config> = {}

  // A missing live config on a machine that carries the backup folder is a restore, not a
  // fresh install: the mirror is read and written back as the live config below.
  const source = existsSync(CONFIG_PATH)
    ? CONFIG_PATH
    : existsSync(CONFIG_BACKUP_PATH)
      ? CONFIG_BACKUP_PATH
      : undefined
  if (source) {
    try {
      fileConfig = JSON.parse(readFileSync(source, 'utf8'))
      if (source === CONFIG_BACKUP_PATH) console.log('[config] restored from backup mirror')
    } catch (e) {
      console.warn('[config] Failed to parse config.json, using defaults:', e)
    }
  }

  // Only when the file carries no value of its own, so a chosen one always survives.
  let defaults: Config = DEFAULT_CONFIG
  if (fileConfig.carName === undefined) {
    defaults = { ...defaults, carName: carNameFromHost() }
  }
  if (fileConfig.projectionWidth === undefined && fileConfig.projectionHeight === undefined) {
    const panel = sysfsPanelGeometry()
    if (panel) {
      // Panels above 720p scale into 1280x720 keeping their aspect: every phone
      // handles that, anything larger is a deliberate settings choice.
      const scale = Math.min(1280 / panel.widthPx, 720 / panel.heightPx, 1)
      const w = 2 * Math.round((panel.widthPx * scale) / 2)
      const h = 2 * Math.round((panel.heightPx * scale) / 2)
      console.log(`[config] projection defaults from the panel: ${w}x${h}`)
      defaults = {
        ...defaults,
        projectionWidth: w,
        projectionHeight: h,
        clusterWidth: w,
        clusterHeight: h
      }
    }
  }

  const merged = validate(fileConfig, defaults)

  const pass = merged.wifiPassword
  if (pass.length < WIFI_PASSWORD_MIN || pass.length > WIFI_PASSWORD_MAX) {
    console.warn(
      `[config] wifiPassword is ${pass.length} characters, hostapd needs ` +
        `${WIFI_PASSWORD_MIN}..${WIFI_PASSWORD_MAX}, falling back to the default`
    )
    merged.wifiPassword = DEFAULT_CONFIG.wifiPassword
  }

  if (!isPagePath(merged.startPage)) {
    console.warn(`[config] startPage ${merged.startPage} is no page, falling back to the default`)
    merged.startPage = DEFAULT_CONFIG.startPage
  }

  const needWrite =
    !existsSync(CONFIG_PATH) || JSON.stringify(fileConfig) !== JSON.stringify(configForFile(merged))

  if (needWrite) {
    writeConfig(merged)
    console.log('[config] Written corrected config.json')
  }

  return merged
}
