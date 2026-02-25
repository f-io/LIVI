import { SettingsNode } from '../types'
import { ExtraConfig } from '../../../../main/Globals'
import { IconUploader } from '../../components/pages/settings/pages/system/iconUploader/IconUploader'

export const appearanceSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'appearance',
  label: 'Appearance',
  labelKey: 'settings.appearance',
  path: '',
  children: [
    {
      type: 'checkbox',
      label: 'Darkmode',
      labelKey: 'settings.nightMode',
      path: 'nightMode'
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
