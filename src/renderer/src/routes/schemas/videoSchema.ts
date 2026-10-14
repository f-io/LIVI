import type { Config } from '@shared/types'
import {
  AREA_STEP,
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
import { SettingsNode } from '../types'

export const videoSchema: SettingsNode<Config> = {
  type: 'route',
  route: 'video',
  label: 'Video',
  labelKey: 'settings.video',
  icon: 'video',
  path: '',
  children: [
    {
      type: 'route',
      label: 'Main Video',
      labelKey: 'settings.mainVideo',
      icon: 'mainVideo',
      route: 'mainVideo',
      path: '',
      children: [
        {
          type: 'number',
          label: 'Width',
          labelKey: 'settings.projectionWidth',
          icon: 'width',
          path: 'projectionWidth',
          min: MIN_WIDTH,
          max: MAX_WIDTH,
          step: 1,
          displayValue: true,
          page: {
            title: 'Main Video Width',
            labelTitle: 'settings.projectionWidth'
          }
        },
        {
          type: 'number',
          label: 'Height',
          labelKey: 'settings.projectionHeight',
          icon: 'height',
          path: 'projectionHeight',
          min: MIN_HEIGHT,
          max: MAX_HEIGHT,
          step: 1,
          displayValue: true,
          page: {
            title: 'Main Video Height',
            labelTitle: 'settings.projectionHeight'
          }
        },
        {
          type: 'number',
          label: 'FPS',
          labelKey: 'settings.projectionFps',
          icon: 'fps',
          path: 'projectionFps',
          min: MIN_FPS,
          max: MAX_FPS,
          step: 1,
          displayValue: true,
          page: {
            title: 'Main Video FPS',
            labelTitle: 'settings.projectionFps'
          }
        },
        {
          type: 'number',
          label: 'DPI',
          labelKey: 'settings.projectionDpi',
          icon: 'dpi',
          path: 'projectionDpi',
          min: MIN_DPI,
          max: MAX_DPI,
          step: 1,
          displayValue: true,
          page: {
            title: 'Main Video DPI',
            labelTitle: 'settings.projectionDpi'
          }
        },
        {
          type: 'route',
          label: 'View Area',
          labelKey: 'settings.viewArea',
          icon: 'viewArea',
          route: 'viewArea',
          path: '',
          children: [
            {
              type: 'number',
              label: 'Top',
              labelKey: 'settings.top',
              icon: 'edgeTop',
              path: 'projectionViewAreaTop',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen View Area Top',
                labelTitle: 'settings.top'
              }
            },
            {
              type: 'number',
              label: 'Bottom',
              labelKey: 'settings.bottom',
              icon: 'edgeBottom',
              path: 'projectionViewAreaBottom',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen View Area Bottom',
                labelTitle: 'settings.bottom'
              }
            },
            {
              type: 'number',
              label: 'Left',
              labelKey: 'settings.left',
              icon: 'edgeLeft',
              path: 'projectionViewAreaLeft',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen View Area Left',
                labelTitle: 'settings.left'
              }
            },
            {
              type: 'number',
              label: 'Right',
              labelKey: 'settings.right',
              icon: 'edgeRight',
              path: 'projectionViewAreaRight',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen View Area Right',
                labelTitle: 'settings.right'
              }
            }
          ]
        },
        {
          type: 'route',
          label: 'Safe Area',
          labelKey: 'settings.safeArea',
          icon: 'safeArea',
          route: 'safeArea',
          path: '',
          children: [
            {
              type: 'number',
              label: 'Top',
              labelKey: 'settings.top',
              icon: 'edgeTop',
              path: 'projectionSafeAreaTop',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Top',
                labelTitle: 'settings.top'
              }
            },
            {
              type: 'number',
              label: 'Bottom',
              labelKey: 'settings.bottom',
              icon: 'edgeBottom',
              path: 'projectionSafeAreaBottom',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Bottom',
                labelTitle: 'settings.bottom'
              }
            },
            {
              type: 'number',
              label: 'Left',
              labelKey: 'settings.left',
              icon: 'edgeLeft',
              path: 'projectionSafeAreaLeft',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Left',
                labelTitle: 'settings.left'
              }
            },
            {
              type: 'number',
              label: 'Right',
              labelKey: 'settings.right',
              icon: 'edgeRight',
              path: 'projectionSafeAreaRight',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Main Screen Safe Area Right',
                labelTitle: 'settings.right'
              }
            },
            {
              type: 'checkbox',
              label: 'Draw Outside',
              labelKey: 'settings.drawOutside',
              icon: 'drawOutside',
              path: 'projectionSafeAreaDrawOutside'
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Cluster Video',
      labelKey: 'settings.clusterVideo',
      icon: 'clusterVideo',
      route: 'clusterVideo',
      path: '',
      children: [
        {
          type: 'number',
          label: 'Width',
          labelKey: 'settings.clusterWidth',
          icon: 'width',
          path: 'clusterWidth',
          min: MIN_WIDTH,
          max: MAX_WIDTH,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Video Width',
            labelTitle: 'settings.clusterWidth'
          }
        },
        {
          type: 'number',
          label: 'Height',
          labelKey: 'settings.clusterHeight',
          icon: 'height',
          path: 'clusterHeight',
          min: MIN_HEIGHT,
          max: MAX_HEIGHT,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Video Height',
            labelTitle: 'settings.clusterHeight'
          }
        },
        {
          type: 'number',
          label: 'FPS',
          labelKey: 'settings.clusterFps',
          icon: 'fps',
          path: 'clusterFps',
          min: MIN_FPS,
          max: MAX_FPS,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Video FPS',
            labelTitle: 'settings.clusterFps'
          }
        },
        {
          type: 'number',
          label: 'DPI',
          labelKey: 'settings.clusterDpi',
          icon: 'dpi',
          path: 'clusterDpi',
          min: MIN_DPI,
          max: MAX_DPI,
          step: 1,
          displayValue: true,
          page: {
            title: 'Cluster Video DPI',
            labelTitle: 'settings.clusterDpi'
          }
        },
        {
          type: 'route',
          label: 'View Area',
          labelKey: 'settings.viewArea',
          icon: 'viewArea',
          route: 'viewArea',
          path: '',
          children: [
            {
              type: 'number',
              label: 'Top',
              labelKey: 'settings.top',
              icon: 'edgeTop',
              path: 'clusterViewAreaTop',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen View Area Top',
                labelTitle: 'settings.top'
              }
            },
            {
              type: 'number',
              label: 'Bottom',
              labelKey: 'settings.bottom',
              icon: 'edgeBottom',
              path: 'clusterViewAreaBottom',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen View Area Bottom',
                labelTitle: 'settings.bottom'
              }
            },
            {
              type: 'number',
              label: 'Left',
              labelKey: 'settings.left',
              icon: 'edgeLeft',
              path: 'clusterViewAreaLeft',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen View Area Left',
                labelTitle: 'settings.left'
              }
            },
            {
              type: 'number',
              label: 'Right',
              labelKey: 'settings.right',
              icon: 'edgeRight',
              path: 'clusterViewAreaRight',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen View Area Right',
                labelTitle: 'settings.right'
              }
            }
          ]
        },
        {
          type: 'route',
          label: 'Safe Area',
          labelKey: 'settings.safeArea',
          icon: 'safeArea',
          route: 'safeArea',
          path: '',
          children: [
            {
              type: 'number',
              label: 'Top',
              labelKey: 'settings.top',
              icon: 'edgeTop',
              path: 'clusterSafeAreaTop',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Top',
                labelTitle: 'settings.top'
              }
            },
            {
              type: 'number',
              label: 'Bottom',
              labelKey: 'settings.bottom',
              icon: 'edgeBottom',
              path: 'clusterSafeAreaBottom',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_HEIGHT,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Bottom',
                labelTitle: 'settings.bottom'
              }
            },
            {
              type: 'number',
              label: 'Left',
              labelKey: 'settings.left',
              icon: 'edgeLeft',
              path: 'clusterSafeAreaLeft',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Left',
                labelTitle: 'settings.left'
              }
            },
            {
              type: 'number',
              label: 'Right',
              labelKey: 'settings.right',
              icon: 'edgeRight',
              path: 'clusterSafeAreaRight',
              min: SAFE_AREA_MIN,
              max: SAFE_AREA_MAX_WIDTH,
              step: AREA_STEP,
              displayValue: true,
              page: {
                title: 'Cluster Screen Safe Area Right',
                labelTitle: 'settings.right'
              }
            }
          ]
        }
      ]
    },
    {
      type: 'checkbox',
      label: 'CarPlay HLS Video Playback',
      labelKey: 'settings.carPlayHlsVideo',
      icon: 'media',
      path: 'carPlayHlsVideo'
    }
  ]
}
