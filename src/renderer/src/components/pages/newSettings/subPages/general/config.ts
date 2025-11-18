import { GeneralSettingKey } from './types'
import { SETTINGS_DIALOGS } from '../types'

export const generalSettingsUIConfig: Record<
  GeneralSettingKey,
  {
    label: string
    dialog?: SETTINGS_DIALOGS
    display?: (state: Record<GeneralSettingKey, number>) => string | number
  }
> = {
  carName: {
    label: 'Car name',
    dialog: SETTINGS_DIALOGS.CAR_NAME,
    display: (state) => state.carName
  },
  oemName: {
    label: 'UI label name',
    dialog: SETTINGS_DIALOGS.OEM_NAME,
    display: (state) => state.oemName
  },
  autoPlay: { label: 'Auto play music' },
  audioTransferMode: { label: 'Audio' },
  nightMode: { label: 'Dark mode' },
  kiosk: { label: 'Fullscreen' }
}
