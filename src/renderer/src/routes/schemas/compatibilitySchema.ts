import type { Config } from '@shared/types'
import type { SettingsNode } from '../types'

export const compatibilitySchema: SettingsNode<Config> = {
  type: 'route',
  route: 'compatibility',
  label: 'Compatibility',
  icon: 'compatibility',
  path: '',
  children: [
    {
      type: 'checkbox',
      label: 'Disable energy sensors (23, 25, 26)',
      path: 'aaDisableEnergySensors'
    }
  ]
}
