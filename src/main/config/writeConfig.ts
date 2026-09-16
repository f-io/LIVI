import type { Config } from '@shared/types'
import { writeFileAtomic } from '@shared/utils'
import { mkdirSync } from 'fs'
import { dirname } from 'path'
import { CONFIG_BACKUP_PATH, CONFIG_PATH } from './paths'

/** The live config and its mirror in the backup folder, always written together. A failed
 *  mirror never blocks the live write: the app keeps running, the backup is just stale. */
export function writeConfig(config: Config): void {
  const json = JSON.stringify(config, null, 2)
  writeFileAtomic(CONFIG_PATH, json)
  try {
    mkdirSync(dirname(CONFIG_BACKUP_PATH), { recursive: true })
    writeFileAtomic(CONFIG_BACKUP_PATH, json)
  } catch (e) {
    console.warn('[config] backup mirror failed:', e)
  }
}
