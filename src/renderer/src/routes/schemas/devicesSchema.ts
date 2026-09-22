import type { Config } from '@shared/types'
import { RouteNode } from '../types'

export const devicesSchema: RouteNode<Config> = {
  type: 'route',
  route: 'devices',
  label: 'Devices',
  labelKey: 'settings.devices',
  icon: 'devices',
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
