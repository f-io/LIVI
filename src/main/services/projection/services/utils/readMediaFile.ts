import fs from 'fs'
import { DEFAULT_MEDIA_DATA_RESPONSE } from '../constants'
import { PersistedMediaFile } from '../types'

export function readMediaFile(filePath: string): PersistedMediaFile {
  try {
    const raw = fs.readFileSync(filePath, 'utf8')
    return JSON.parse(raw) as PersistedMediaFile
  } catch (error) {
    console.log('Error: readMediaFile', error)
    return DEFAULT_MEDIA_DATA_RESPONSE
  }
}
