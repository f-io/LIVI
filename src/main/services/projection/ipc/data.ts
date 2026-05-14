import { registerIpcHandle } from '@main/ipc/register'
import { app } from 'electron'
import fs from 'fs'
import path from 'path'
import {
  DEFAULT_MEDIA_DATA_RESPONSE,
  DEFAULT_NAVIGATION_DATA_RESPONSE
} from '../services/constants'
import { readMediaFile } from '../services/utils/readMediaFile'
import { readNavigationFile } from '../services/utils/readNavigationFile'

export function registerDataIpc(): void {
  registerIpcHandle('projection-media-read', async () => {
    try {
      const file = path.join(app.getPath('userData'), 'mediaData.json')
      if (!fs.existsSync(file)) {
        console.log('[projection-media-read] Error: ENOENT: no such file or directory')
        return DEFAULT_MEDIA_DATA_RESPONSE
      }
      return readMediaFile(file)
    } catch (error) {
      console.log('[projection-media-read]', error)
      return DEFAULT_MEDIA_DATA_RESPONSE
    }
  })

  registerIpcHandle('projection-navigation-read', async () => {
    try {
      const file = path.join(app.getPath('userData'), 'navigationData.json')
      if (!fs.existsSync(file)) return DEFAULT_NAVIGATION_DATA_RESPONSE
      return readNavigationFile(file)
    } catch (error) {
      console.log('[projection-navigation-read]', error)
      return DEFAULT_NAVIGATION_DATA_RESPONSE
    }
  })
}
