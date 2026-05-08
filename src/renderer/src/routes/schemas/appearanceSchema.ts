import type { ExtraConfig } from '@shared/types'
import { IconUploader } from '../../components/pages/settings/pages/system/iconUploader/IconUploader'
import { SettingsNode } from '../types'

export const appearanceSchema: SettingsNode<ExtraConfig> = {
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
      label: 'Phone Appearance',
      labelKey: 'settings.phoneAppearance',
      path: 'appearanceMode',
      displayValue: true,
      options: [
        { label: 'Auto', labelKey: 'settings.phoneAppearanceAuto', value: 'auto' },
        { label: 'Day', labelKey: 'settings.phoneAppearanceDay', value: 'day' },
        { label: 'Night', labelKey: 'settings.phoneAppearanceNight', value: 'night' }
      ],
      page: {
        title: 'Phone Appearance',
        labelTitle: 'settings.phoneAppearance',
        description:
          'Light / dark appearance for the connected phone (Android Auto / CarPlay). Auto follows vehicle data (CAN, ambient sensor, dongle hint). Day or Night force the corresponding appearance on the phone when it connects.',
        labelDescription: 'settings.phoneAppearanceDescription'
      }
    },
    {
      type: 'color',
      label: 'Primary Color Dark',
      labelKey: 'settings.primaryColorDark',
      path: 'primaryColorDark'
    },
    {
      type: 'color',
      label: 'Highlight Color Dark',
      labelKey: 'settings.highlightColorDark',
      path: 'highlightColorDark'
    },
    {
      type: 'color',
      label: 'Primary Color Light',
      labelKey: 'settings.primaryColorLight',
      path: 'primaryColorLight'
    },
    {
      type: 'color',
      label: 'Highlight Color Light',
      labelKey: 'settings.highlightColorLight',
      path: 'highlightColorLight'
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
