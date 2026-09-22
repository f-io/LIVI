import { app } from 'electron'
import { homedir } from 'os'
import { join } from 'path'

export const CONFIG_PATH = join(app.getPath('userData'), 'config.json')

/** The one folder to copy when moving to a new machine: a mirror of config.json and the
 *  dongle backups taken before flashing. Everything else LIVI writes can be regenerated. */
export const BACKUP_DIR =
  process.platform === 'darwin'
    ? join(app.getPath('userData'), 'backup')
    : join(process.env.XDG_DATA_HOME || join(homedir(), '.local', 'share'), 'LIVI')

export const CONFIG_BACKUP_PATH = join(BACKUP_DIR, 'config.json')
