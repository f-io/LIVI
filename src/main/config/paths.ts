import { app } from 'electron'
import { join } from 'path'

export const CONFIG_PATH = join(app.getPath('userData'), 'config.json')
