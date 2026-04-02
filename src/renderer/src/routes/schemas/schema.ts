import type { ExtraConfig } from '@shared/types'
import { generateRoutes } from '../../utils/generateRoutes'
import { SettingsNode } from '../types'
import { appearanceSchema } from './appearanceSchema'
import { audioSchema } from './audioSchema'
import { devicesSchema } from './devicesSchema'
import { generalSchema } from './generalSchema'
import { systemSchema } from './systemSchema'
import { videoSchema } from './videoSchema'

export const settingsSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'new-settings',
  label: 'Settings', // TODO deleted in favor of i18n
  labelKey: 'settings.settingsTitle',
  path: 'settings',
  children: [devicesSchema, generalSchema, audioSchema, videoSchema, appearanceSchema, systemSchema]
}

export const settingsRoutes = generateRoutes(settingsSchema)
