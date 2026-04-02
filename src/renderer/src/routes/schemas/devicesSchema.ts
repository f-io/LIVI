import type { ExtraConfig } from '@shared/types'
import { SettingsNode } from '../types'

export const devicesSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'devices',
  label: 'Devices',
  labelKey: 'settings.devices',
  path: '',
  children: [
    {
      type: 'btDeviceList',
      label: 'Devices',
      labelKey: 'settings.devices',
      path: 'bluetoothPairedDevices'
    }
  ]
}
