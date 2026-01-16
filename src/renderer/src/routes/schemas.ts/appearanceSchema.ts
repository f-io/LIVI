import { SettingsNode } from '../types'
import { ExtraConfig } from '../../../../main/Globals'
import { IconUploader } from '../../components/pages/settings/pages/system/iconUploader/IconUploader'

export const appearanceSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'appearance',
  label: 'Appearance',
  path: '',
  children: [
    {
      type: 'checkbox',
      label: 'Darkmode',
      path: 'nightMode'
    },
    {
      type: 'color',
      label: 'Primary Color Dark',
      path: 'primaryColorDark'
    },
    {
      type: 'color',
      label: 'Highlight Color Dark',
      path: 'highlightColorDark'
    },
    {
      type: 'color',
      label: 'Primary Color Light',
      path: 'primaryColorLight'
    },
    {
      type: 'color',
      label: 'Highlight Color Light',
      path: 'highlightColorLight'
    },
    {
      type: 'route',
      label: 'UI Icon',
      route: 'ui-icon',
      path: '',
      children: [
        { type: 'custom', label: 'UI Icon', path: 'dongleIcon180', component: IconUploader }
      ]
    }
  ]
}
