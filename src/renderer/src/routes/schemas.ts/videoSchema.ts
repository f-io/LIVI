import { SettingsNode } from '../types'
import { ExtraConfig } from '@main/Globals'

export const videoSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'video',
  label: 'Video',
  path: '',
  children: [
    {
      type: 'number',
      label: 'Width',
      path: 'width',
      displayValue: true,
      page: { title: 'Width', description: 'Stream width in px' }
    },
    {
      type: 'number',
      label: 'Height',
      path: 'height',
      displayValue: true,
      page: { title: 'Height', description: 'Stream height in px' }
    },
    {
      type: 'number',
      label: 'FPS',
      path: 'fps',
      displayValue: true,
      page: { title: 'FPS', description: 'FPS' }
    }
  ]
}
