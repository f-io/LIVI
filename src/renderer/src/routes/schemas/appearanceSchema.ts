import type { Config } from '@shared/types'
import { ColorCalibration } from '../../components/pages/settings/pages/displayCalibration/ColorCalibration'
import { ContrastGammaCalibration } from '../../components/pages/settings/pages/displayCalibration/ContrastGammaCalibration'
import { SettingsNode, ValueTransform } from '../types'

const brightnessValueTransform: ValueTransform<number | undefined, number> = {
  toView: (v) => Math.round((v ?? 1) * 100),
  fromView: (v, prev) => {
    const next = v / 100
    if (!Number.isFinite(next)) return prev ?? 1
    return next
  },
  format: (v) => `${v} %`
}

export const appearanceSchema: SettingsNode<Config> = {
  type: 'route',
  route: 'appearance',
  label: 'Appearance',
  labelKey: 'settings.appearance',
  icon: 'appearance',
  path: '',
  children: [
    {
      type: 'slider',
      label: 'Display Brightness',
      labelKey: 'settings.displayBrightness',
      icon: 'displayBrightness',
      path: 'displayBrightness',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: brightnessValueTransform,
      page: {
        title: 'Display Brightness',
        labelTitle: 'settings.displayBrightness'
      }
    },
    {
      type: 'checkbox',
      label: 'Auto Brightness',
      labelKey: 'settings.displayBrightnessAuto',
      icon: 'displayBrightnessAuto',
      path: 'displayBrightnessAuto'
    },
    {
      type: 'checkbox',
      label: 'Dark Mode',
      labelKey: 'settings.darkMode',
      icon: 'darkMode',
      path: 'darkMode'
    },
    {
      type: 'select',
      label: 'Phone Appearance',
      labelKey: 'settings.phoneAppearance',
      icon: 'phoneAppearance',
      path: 'appearanceMode',
      displayValue: true,
      options: [
        { label: 'Auto', labelKey: 'settings.phoneAppearanceAuto', value: 'auto' },
        { label: 'Day', labelKey: 'settings.phoneAppearanceDay', value: 'day' },
        { label: 'Night', labelKey: 'settings.phoneAppearanceNight', value: 'night' }
      ],
      page: {
        title: 'Phone Appearance',
        labelTitle: 'settings.phoneAppearance'
      }
    },
    {
      type: 'route',
      label: 'UI Colors',
      route: 'ui-colors',
      icon: 'uiColors',
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
      icon: 'contrastGamma',
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
      icon: 'displayColor',
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
      icon: 'uiIcon',
      route: 'ui-icon',
      path: '',
      children: []
    }
  ]
}
