import fs from 'fs'
import { DEFAULT_NAVIGATION_DATA_RESPONSE } from '../constants'
import { PersistedNavigationFile } from '../types'

export function readNavigationFile(filePath: string): PersistedNavigationFile {
  try {
    const raw = fs.readFileSync(filePath, 'utf8')
    return JSON.parse(raw) as PersistedNavigationFile
  } catch (error) {
    console.log('Error: readNavigationFile', error)
    return DEFAULT_NAVIGATION_DATA_RESPONSE
  }
}
