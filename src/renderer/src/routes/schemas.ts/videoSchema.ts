import { SettingsNode } from '../types'
import { ExtraConfig } from '@main/Globals'
import { Camera } from '../../components/pages/settings/pages/camera'

export const videoSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'video',
  label: 'Video',
  labelKey: 'settings.video',
  path: '',
  children: [
    {
      type: 'number',
      label: 'Width',
      labelKey: 'settings.width',
      path: 'width',
      displayValue: true,
      page: {
        title: 'Width',
        labelTitle: 'settings.width',
        description: 'Stream width in px',
        labelDescription: 'settings.widthDescription'
      }
    },
    {
      type: 'number',
      label: 'Height',
      labelKey: 'settings.height',
      path: 'height',
      displayValue: true,
      page: {
        title: 'Height',
        labelTitle: 'settings.height',
        description: 'Stream height in px',
        labelDescription: 'settings.heightDescription'
      }
    },
    {
      type: 'number',
      label: 'FPS',
      labelKey: 'settings.fps',
      path: 'fps',
      displayValue: true,
      page: {
        title: 'FPS',
        labelTitle: 'settings.fps',
        description: 'FPS',
        labelDescription: 'settings.fpsDescription'
      }
    },
    {
      type: 'route',
      label: 'Camera',
      labelKey: 'settings.camera',
      route: 'camera',
      path: '',
      displayValue: true,
      children: [
        {
          path: 'camera',
          type: 'custom',
          label: 'Camera',
          labelKey: 'settings.camera',
          component: Camera
        }
      ]
    }
  ]
}
