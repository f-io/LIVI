import type { ExtraConfig } from '@shared/types'
import {
  MAX_DPI,
  MAX_FPS,
  MAX_HEIGHT,
  MAX_WIDTH,
  MIN_DPI,
  MIN_FPS,
  MIN_HEIGHT,
  MIN_WIDTH,
  SAFE_AREA_MAX_HEIGHT,
  SAFE_AREA_MAX_WIDTH,
  SAFE_AREA_MIN
} from '../../components/pages/settings/constants'
import { Camera } from '../../components/pages/settings/pages/camera'
import { SettingsNode } from '../types'

export const videoSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'video',
  label: 'Video',
  labelKey: 'settings.video',
  path: '',
  children: [
    {
      type: 'route',
      label: 'Main Screen',
      labelKey: 'settings.mainScreen',
      route: 'mainScreen',
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
            title: 'Main Screen Width',
            labelTitle: 'settings.width',
            description: 'Main stream width in px',
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
            title: 'Main Screen Height',
            labelTitle: 'settings.height',
            description: 'Main stream height in px',
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
            title: 'Main Screen FPS',
            labelTitle: 'settings.fps',
            description: 'Main stream FPS',
            labelDescription: 'settings.fpsDescription'
          }
        },
        {
          type: 'number',
          label: 'DPI',
          labelKey: 'settings.dpi',
          path: 'dpi',
          min: MIN_DPI,
          max: MAX_DPI,
          step: 1,
          displayValue: true,
          page: {
            title: 'Main Screen DPI',
            labelTitle: 'settings.dpi',
            description: 'Main stream DPI (0 = auto)',
            labelDescription: 'settings.dpiDescription'
          }
        },
        {
          type: 'route',
          label: 'Safe Area',
          labelKey: 'settings.safeArea',
          route: 'safeArea',
          path: '',
          children: [
            {
              type: 'number',
              label: 'Top',
              labelKey: 'settings.top',
              path: 'projectionSafeAreaTop',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Top',
                labelTitle: 'settings.top',
                description: 'Top inset in px',
                labelDescription: 'settings.safeAreaTopDescription'
              }
            },
            {
              type: 'number',
              label: 'Bottom',
              labelKey: 'settings.bottom',
              path: 'projectionSafeAreaBottom',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Bottom',
                labelTitle: 'settings.bottom',
                description: 'Bottom inset in px',
                labelDescription: 'settings.safeAreaBottomDescription'
              }
            },
            {
              type: 'number',
              label: 'Left',
              labelKey: 'settings.left',
              path: 'projectionSafeAreaLeft',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Left',
                labelTitle: 'settings.left',
                description: 'Left inset in px',
                labelDescription: 'settings.safeAreaLeftDescription'
              }
            },
            {
              type: 'number',
              label: 'Right',
              labelKey: 'settings.right',
              path: 'projectionSafeAreaRight',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Right',
                labelTitle: 'settings.right',
                description: 'Right inset in px',
                labelDescription: 'settings.safeAreaRightDescription'
              }
            },
            {
              type: 'checkbox',
              label: 'Draw Outside',
              labelKey: 'settings.drawOutside',
              path: 'projectionSafeAreaDrawOutside'
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Cluster Screen',
      labelKey: 'settings.clusterScreen',
      route: 'clusterScreen',
      path: '',
      children: [
        {
          type: 'number',
          label: 'Width',
          labelKey: 'settings.width',
          path: 'clusterWidth',
          min: MIN_WIDTH,
          max: MAX_WIDTH,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Screen Width',
            labelTitle: 'settings.width',
            description: 'Cluster screen width in px',
            labelDescription: 'settings.clusterScreenWidthDescription'
          }
        },
        {
          type: 'number',
          label: 'Height',
          labelKey: 'settings.height',
          path: 'clusterHeight',
          min: MIN_HEIGHT,
          max: MAX_HEIGHT,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Screen Height',
            labelTitle: 'settings.height',
            description: 'Cluster screen height in px',
            labelDescription: 'settings.clusterScreenHeightDescription'
          }
        },
        {
          type: 'number',
          label: 'FPS',
          labelKey: 'settings.fps',
          path: 'clusterFps',
          min: MIN_FPS,
          max: MAX_FPS,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Screen FPS',
            labelTitle: 'settings.fps',
            description: 'Cluster screen FPS',
            labelDescription: 'settings.clusterScreenFpsDescription'
          }
        },
        {
          type: 'number',
          label: 'DPI',
          labelKey: 'settings.dpi',
          path: 'clusterDpi',
          min: MIN_DPI,
          max: MAX_DPI,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Screen DPI',
            labelTitle: 'settings.dpi',
            description: 'Cluster screen DPI (0 = auto)',
            labelDescription: 'settings.clusterScreenDpiDescription'
          }
        },
        {
          type: 'route',
          label: 'Safe Area',
          labelKey: 'settings.safeArea',
          route: 'safeArea',
          path: '',
          children: [
            {
              type: 'number',
              label: 'Top',
              labelKey: 'settings.top',
              path: 'clusterSafeAreaTop',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Top',
                labelTitle: 'settings.top',
                description: 'Top inset in px',
                labelDescription: 'settings.clusterSafeAreaTopDescription'
              }
            },
            {
              type: 'number',
              label: 'Bottom',
              labelKey: 'settings.bottom',
              path: 'clusterSafeAreaBottom',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: 1,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Bottom',
                labelTitle: 'settings.bottom',
                description: 'Bottom inset in px',
                labelDescription: 'settings.clusterSafeAreaBottomDescription'
              }
            },
            {
              type: 'number',
              label: 'Left',
              labelKey: 'settings.left',
              path: 'clusterSafeAreaLeft',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Left',
                labelTitle: 'settings.left',
                description: 'Left inset in px',
                labelDescription: 'settings.clusterSafeAreaLeftDescription'
              }
            },
            {
              type: 'number',
              label: 'Right',
              labelKey: 'settings.right',
              path: 'clusterSafeAreaRight',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: 1,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Right',
                labelTitle: 'settings.right',
                description: 'Right inset in px',
                labelDescription: 'settings.clusterSafeAreaRightDescription'
              }
            },
            {
              type: 'checkbox',
              label: 'Draw Outside',
              labelKey: 'settings.drawOutside',
              path: 'clusterSafeAreaDrawOutside'
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Reverse Camera',
      labelKey: 'settings.reverseCamera',
      route: 'camera',
      path: '',
      displayValue: true,
      children: [
        {
          type: 'checkbox',
          label: 'Camera Tab',
          labelKey: 'settings.cameraTab',
          path: 'cameraEnabled'
        },
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
    },
    {
      type: 'checkbox',
      label: 'HW Acceleration',
      labelKey: 'settings.hwAcceleration',
      path: 'hwAcceleration'
    }
  ]
}
