import { SoftwareUpdate } from '../../components/pages/settings/pages/system/softwareUpdate/SoftwareUpdate'
import { USBDongle } from '../../components/pages/settings/pages/system/usbDongle/USBDongle'
import { About } from '../../components/pages/settings/pages/system/About'
import { Restart } from '../../components/pages/settings/pages/system/Restart'
import { PowerOff } from '../../components/pages/settings/pages/system/PowerOff'
import type { SettingsNode } from '../types'
import type { ExtraConfig } from '@main/Globals'

export const systemSchema: SettingsNode<ExtraConfig> = {
  route: 'system',
  label: 'System',
  type: 'route',
  path: '',
  children: [
    {
      type: 'route',
      label: 'About',
      route: 'about',
      path: '',
      children: [{ type: 'custom', label: 'About', path: 'carName', component: About }]
    },
    {
      type: 'route',
      label: 'USB Dongle',
      route: 'USBDongle',
      path: '',
      children: [{ type: 'custom', label: 'USB Dongle', path: 'carName', component: USBDongle }]
    },
    {
      type: 'route',
      label: 'Software Update',
      route: 'softwareUpdate',
      path: '',
      children: [
        { type: 'custom', label: 'Software Update', path: 'carName', component: SoftwareUpdate }
      ]
    },
    {
      type: 'route',
      label: 'Restart System',
      route: 'restart',
      path: '',
      children: [{ type: 'custom', label: 'Restart System', path: 'carName', component: Restart }]
    },
    {
      type: 'route',
      label: 'Power Off',
      route: 'poweroff',
      path: '',
      children: [{ type: 'custom', label: 'Power Off', path: 'carName', component: PowerOff }]
    }
  ]
}
