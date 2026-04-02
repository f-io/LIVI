import type { ExtraConfig } from '@shared/types'
import { About } from '../../components/pages/settings/pages/system/About'
import { Debug } from '../../components/pages/settings/pages/system/debug/Debug'
import { PowerOff } from '../../components/pages/settings/pages/system/PowerOff'
import { Restart } from '../../components/pages/settings/pages/system/Restart'
import { SoftwareUpdate } from '../../components/pages/settings/pages/system/softwareUpdate/SoftwareUpdate'
import { USBDongle } from '../../components/pages/settings/pages/system/usbDongle/USBDongle'
import type { SettingsNode } from '../types'

export const systemSchema: SettingsNode<ExtraConfig> = {
  route: 'system',
  label: 'System',
  labelKey: 'settings.system',
  type: 'route',
  path: '',
  children: [
    {
      type: 'route',
      label: 'About',
      labelKey: 'settings.about',
      route: 'about',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'About',
          labelKey: 'settings.about',
          path: 'carName',
          component: About
        }
      ]
    },
    {
      type: 'route',
      label: 'Debug',
      labelKey: 'settings.debug',
      route: 'debug',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'Debug',
          labelKey: 'settings.debug',
          path: 'carName',
          component: Debug
        }
      ]
    },
    {
      type: 'route',
      label: 'USB Dongle',
      labelKey: 'settings.usbDongle',
      route: 'usbDongle',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'USB Dongle',
          labelKey: 'settings.usbDongle',
          path: 'carName',
          component: USBDongle
        }
      ]
    },
    {
      type: 'route',
      label: 'Software Update',
      labelKey: 'settings.softwareUpdate',
      route: 'softwareUpdate',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'Software Update',
          labelKey: 'settings.softwareUpdate',
          path: 'carName',
          component: SoftwareUpdate
        }
      ]
    },
    {
      type: 'route',
      label: 'Restart System',
      labelKey: 'settings.restartSystem',
      route: 'restart',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'Restart System',
          labelKey: 'settings.restartSystem',
          path: 'carName',
          component: Restart
        }
      ]
    },
    {
      type: 'route',
      label: 'Power Off',
      labelKey: 'settings.powerOff',
      route: 'poweroff',
      path: '',
      children: [
        {
          type: 'custom',
          label: 'Power Off',
          labelKey: 'settings.powerOff',
          path: 'carName',
          component: PowerOff
        }
      ]
    }
  ]
}
