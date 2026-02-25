import { existsSync, readFileSync, writeFileSync } from 'fs'
import { CONFIG_PATH } from './paths'
import { DEFAULT_BINDINGS, ExtraConfig } from '@main/Globals'
import { DEFAULT_CONFIG } from '@main/services/carplay'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from '@main/services/carplay/assets/carIcons'

export function loadConfig(): ExtraConfig {
  let fileConfig: Partial<ExtraConfig> = {}
  if (existsSync(CONFIG_PATH)) {
    try {
      fileConfig = JSON.parse(readFileSync(CONFIG_PATH, 'utf8'))
    } catch (e) {
      console.warn('[config] Failed to parse config.json, using defaults:', e)
      fileConfig = {}
    }
  }

  // Start with defaults
  const merged: ExtraConfig = {
    ...DEFAULT_CONFIG,
    startPage: 'home',
    kiosk: true,
    camera: '',
    cameraMirror: false,
    nightMode: true,
    audioVolume: 0.95,
    navVolume: 0.95,
    siriVolume: 0.95,
    callVolume: 0.95,
    autoSwitchOnStream: false,
    autoSwitchOnPhoneCall: true,
    autoSwitchOnGuidance: true,
    visualAudioDelayMs: 120,
    language: 'en',
    ...fileConfig,
    bindings: { ...DEFAULT_BINDINGS, ...(fileConfig.bindings || {}) }
  } as ExtraConfig

  if (!merged.dongleIcon120) merged.dongleIcon120 = ICON_120_B64
  if (!merged.dongleIcon180) merged.dongleIcon180 = ICON_180_B64
  if (!merged.dongleIcon256) merged.dongleIcon256 = ICON_256_B64

  const needWrite =
    !existsSync(CONFIG_PATH) || JSON.stringify(fileConfig) !== JSON.stringify(merged)

  if (needWrite) {
    try {
      writeFileSync(CONFIG_PATH, JSON.stringify(merged, null, 2))
      console.log('[config] Written complete config.json with all defaults')
    } catch (e) {
      console.warn('[config] Failed to write config.json:', e)
    }
  }

  return merged
}
