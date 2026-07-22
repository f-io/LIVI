import { hostname } from 'node:os'
import type { Config } from '@shared/types'
import { DEFAULT_CONFIG } from '@shared/types'
import { CAR_NAME_MAX } from '@shared/types/Config'
import { existsSync, readFileSync, writeFileSync } from 'fs'
import { CONFIG_PATH } from './paths'
import { validate } from './validateConfig'

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

  if (existsSync(CONFIG_PATH)) {
    try {
      fileConfig = JSON.parse(readFileSync(CONFIG_PATH, 'utf8'))
    } catch (e) {
      console.warn('[config] Failed to parse config.json, using defaults:', e)
    }
  }

  // Only when the file carries no name of its own, so a chosen one always survives.
  const defaults =
    fileConfig.carName === undefined
      ? { ...DEFAULT_CONFIG, carName: carNameFromHost() }
      : DEFAULT_CONFIG

  const merged = validate(fileConfig, defaults)

  const needWrite =
    !existsSync(CONFIG_PATH) || JSON.stringify(fileConfig) !== JSON.stringify(merged)

  if (needWrite) {
    writeFileSync(CONFIG_PATH, JSON.stringify(merged, null, 2))
    console.log('[config] Written corrected config.json')
  }

  return merged
}
