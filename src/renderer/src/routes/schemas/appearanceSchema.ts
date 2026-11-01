import type { Config } from '@shared/types'
import { ColorCalibration } from '../../components/pages/settings/pages/displayCalibration/ColorCalibration'
import { ContrastGammaCalibration } from '../../components/pages/settings/pages/displayCalibration/ContrastGammaCalibration'
import { IconUploader } from '../../components/pages/settings/pages/system/iconUploader/IconUploader'
import { SettingsNode } from '../types'

export const appearanceSchema: SettingsNode<Config> = {
  type: 'route',
  route: 'appearance',
  label: 'Appearance',
  labelKey: 'settings.appearance',
  path: '',
  children: [
    {
      type: 'checkbox',
      label: 'Dark Mode',
      labelKey: 'settings.darkMode',
      path: 'darkMode'
    },
    {
      type: 'select',
      label: 'Day / Night Mode',
      labelKey: 'settings.phoneAppearance',
      path: 'appearanceMode',
      displayValue: true,
      options: [
        { label: 'Sunrise / Sunset', labelKey: 'settings.phoneAppearanceSun', value: 'sun' },
        { label: 'Day', labelKey: 'settings.phoneAppearanceDay', value: 'day' },
        { label: 'Night', labelKey: 'settings.phoneAppearanceNight', value: 'night' },
        { label: 'Auto (sensor)', labelKey: 'settings.phoneAppearanceAuto', value: 'auto' }
      ],
      page: {
        title: 'Day / Night Mode',
        labelTitle: 'settings.phoneAppearance'
      }
    },
    {
      type: 'number',
      label: 'Latitude (no GPS)',
      labelKey: 'settings.sunLatitude',
      path: 'sunLatitude',
      min: -90,
      max: 90,
      step: 0.01,
      displayValue: true,
      page: { title: 'Latitude', labelTitle: 'settings.sunLatitude' }
    },
    {
      type: 'number',
      label: 'Longitude (no GPS)',
      labelKey: 'settings.sunLongitude',
      path: 'sunLongitude',
      min: -180,
      max: 180,
      step: 0.01,
      displayValue: true,
      page: { title: 'Longitude', labelTitle: 'settings.sunLongitude' }
    },
    {
      type: 'route',
      label: 'UI Colors',
      route: 'ui-colors',
      path: '',
      children: [
        {
          type: 'color',
          label: 'Primary Color Dark',
          labelKey: 'settings.primaryColorDark',
          path: 'primaryColorDark',
          displayValue: true,
          page: { title: 'Primary Color Dark', labelTitle: 'settings.primaryColorDark' }
        },
        {
          type: 'color',
          label: 'Highlight Color Dark',
          labelKey: 'settings.highlightColorDark',
          path: 'highlightColorDark',
          displayValue: true,
          page: { title: 'Highlight Color Dark', labelTitle: 'settings.highlightColorDark' }
        },
        {
          type: 'color',
          label: 'Background Color Dark',
          labelKey: 'settings.backgroundColorDark',
          path: 'backgroundColorDark',
          displayValue: true,
          page: { title: 'Background Color Dark', labelTitle: 'settings.backgroundColorDark' }
        },
        {
          type: 'color',
          label: 'Primary Color Light',
          labelKey: 'settings.primaryColorLight',
          path: 'primaryColorLight',
          displayValue: true,
          page: { title: 'Primary Color Light', labelTitle: 'settings.primaryColorLight' }
        },
        {
          type: 'color',
          label: 'Highlight Color Light',
          labelKey: 'settings.highlightColorLight',
          path: 'highlightColorLight',
          displayValue: true,
          page: { title: 'Highlight Color Light', labelTitle: 'settings.highlightColorLight' }
        },
        {
          type: 'color',
          label: 'Background Color Light',
          labelKey: 'settings.backgroundColorLight',
          path: 'backgroundColorLight',
          displayValue: true,
          page: { title: 'Background Color Light', labelTitle: 'settings.backgroundColorLight' }
        }
      ]
    },
    {
      type: 'route',
      label: 'Contrast / Gamma',
      route: 'display-contrast-gamma',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'Contrast / Gamma',
          path: 'displayGamma',
          component: ContrastGammaCalibration
        }
      ]
    },
    {
      type: 'route',
      label: 'Color',
      route: 'display-color',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'Color',
          path: 'displayColorR',
          component: ColorCalibration
        }
      ]
    },
    {
      type: 'route',
      label: 'UI Icon',
      labelKey: 'settings.uiIcon',
      route: 'ui-icon',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'UI Icon',
          labelKey: 'settings.uiIcon',
          path: 'dongleIcon180',
          component: IconUploader
        }
      ]
    }
  ]
}
