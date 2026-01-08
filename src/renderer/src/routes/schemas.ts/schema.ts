import { generateRoutes } from '../../utils/generateRoutes'
import { generalSchema } from './generalSchema'
import { audioSchema } from './audioSchema'
import { videoSchema } from './videoSchema'
import { appearanceSchema } from './appearanceSchema'
import { SettingsNode } from '../types'
import { ExtraConfig } from '../../../../main/Globals'
import { systemSchema } from './systemSchema'

export const settingsSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'new-settings',
  label: 'Settings',
  path: 'settings',
  children: [generalSchema, audioSchema, videoSchema, appearanceSchema, systemSchema]
}

export const settingsRoutes = generateRoutes(settingsSchema)
