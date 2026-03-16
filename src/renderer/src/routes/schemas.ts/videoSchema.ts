import { SettingsNode } from '../types'
import type { ExtraConfig } from '@shared/types'
import { Camera } from '../../components/pages/settings/pages/camera'
import {
  MIN_WIDTH,
  MIN_HEIGHT,
  MAX_WIDTH,
  MAX_HEIGHT,
  MIN_FPS,
  MAX_FPS
} from '../../components/pages/settings/constants'

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
      min: MIN_WIDTH,
      max: MAX_WIDTH,
      step: 1,
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
      min: MIN_HEIGHT,
      max: MAX_HEIGHT,
      step: 1,
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
      min: MIN_FPS,
      max: MAX_FPS,
      step: 1,
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
      label: 'Navigation Screen',
      labelKey: 'settings.navigationScreen',
      route: 'navigationScreen',
      path: '',
      children: [
        {
          type: 'number',
          label: 'Width',
          labelKey: 'settings.width',
          path: 'naviWidth',
          min: MIN_WIDTH,
          max: MAX_WIDTH,
          step: 1,
          displayValue: true,
          page: {
            title: 'Navigation Screen Width',
            labelTitle: 'settings.width',
            description: 'Navigation screen width in px',
            labelDescription: 'settings.navigationScreenWidthDescription'
          }
        },
        {
          type: 'number',
          label: 'Height',
          labelKey: 'settings.height',
          path: 'naviHeight',
          min: MIN_HEIGHT,
          max: MAX_HEIGHT,
          step: 1,
          displayValue: true,
          page: {
            title: 'Navigation Screen Height',
            labelTitle: 'settings.height',
            description: 'Navigation screen height in px',
            labelDescription: 'settings.navigationScreenHeightDescription'
          }
        },
        {
          type: 'number',
          label: 'FPS',
          labelKey: 'settings.fps',
          path: 'naviFps',
          min: MIN_FPS,
          max: MAX_FPS,
          step: 1,
          displayValue: true,
          page: {
            title: 'Navigation Screen FPS',
            labelTitle: 'settings.fps',
            description: 'Navigation screen FPS',
            labelDescription: 'settings.navigationScreenFpsDescription'
          }
        }
      ]
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
          type: 'route',
          label: 'Camera',
          labelKey: 'settings.camera',
          route: 'select',
          path: '',
          children: [
            {
              path: 'camera',
              type: 'custom',
              label: 'Camera',
              labelKey: 'settings.camera',
              component: Camera
            }
          ]
        },
        {
          type: 'checkbox',
          label: 'Mirror',
          labelKey: 'settings.cameraMirror',
          path: 'cameraMirror'
        }
      ]
    }
  ]
}
