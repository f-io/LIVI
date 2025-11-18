import { SETTINGS_DIALOGS } from '../types'
import { SourcesSettingKey } from './types'

export const sourcesSettingsUIConfig: Record<
  SourcesSettingKey,
  {
    label: string
    dialog?: SETTINGS_DIALOGS
    display?: (state: Record<SourcesSettingKey, number>) => string | number
    customVariantType?: string
    customVariantValues?: string[]
  }
> = {
  wifiType: {
    label: 'WiFi',
    customVariantValues: ['2.4ghz', '5ghz']
  },
  micType: {
    label: 'Microphone',
    customVariantValues: ['os', 'box']
  },
  camera: {
    label: 'Camera'
  }
}
