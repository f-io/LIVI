import { AdvancedSettingKey, UiThemeSettingKey } from './types'
import { SETTINGS_DIALOGS } from '../types'

export const advancedSettingsUIConfig: Record<
  AdvancedSettingKey,
  {
    label: string
    dialog?: SETTINGS_DIALOGS
    display?: (state: Record<AdvancedSettingKey, number>) => string | number
  }
> = {
  dpi: {
    label: 'DPI',
    dialog: SETTINGS_DIALOGS.DPI,
    display: (state) => state.dpi
  },
  format: {
    label: 'Format',
    dialog: SETTINGS_DIALOGS.FORMAT,
    display: (state) => state.format
  },
  language: {
    label: 'Language'
  }
}

export const uiThemeSettingsUIConfig: Record<
  UiThemeSettingKey,
  {
    label: string
    dialog?: SETTINGS_DIALOGS
    display?: (state: Record<UiThemeSettingKey, number>) => string | number
  }
> = {
  primaryColor: {
    label: 'Primary Color'
  },
  highlightEditableField: {
    label: 'Highlight Editable Field Color'
  }
}
