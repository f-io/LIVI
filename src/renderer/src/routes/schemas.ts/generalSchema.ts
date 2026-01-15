import { Camera } from '../../components/pages/settings/pages/camera'
import { SettingsNode } from '../types'
import { ExtraConfig } from '../../../../main/Globals'

export const generalSchema: SettingsNode<ExtraConfig> = {
  route: 'general',
  label: 'General',
  type: 'route',
  path: '',
  children: [
    {
      type: 'route',
      route: 'connections',
      label: 'Connections',
      path: '',
      children: [
        {
          type: 'string',
          label: 'Car Name',
          path: 'carName',
          displayValue: true,
          page: {
            title: 'Car Name',
            description: 'The name of the CarPlay device'
          }
        },
        {
          type: 'string',
          label: 'UI Name',
          path: 'oemName',
          displayValue: true,
          page: {
            title: 'UI Name',
            description: 'The name displayed in the CarPlay UI.'
          }
        },
        {
          type: 'checkbox',
          label: 'Auto Connect',
          path: 'autoConn'
        },
        {
          type: 'route',
          route: 'wifi',
          label: 'Wi-Fi',
          path: '',
          children: [
            {
              type: 'select',
              label: 'Wi-Fi Frequency',
              path: 'wifiType',
              displayValue: true,
              options: [
                {
                  label: '2.4 GHz',
                  value: '2.4ghz'
                },
                {
                  label: '5 GHz',
                  value: '5ghz'
                }
              ],
              page: {
                title: 'Wi-Fi Frequency',
                description: 'Wi-Fi frequency selection'
              }
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Sink & Sources',
      route: 'sinkandsources',
      path: '',
      children: [
        {
          type: 'route',
          label: 'Camera',
          route: 'camera',
          path: '',
          displayValue: true,
          children: [
            {
              path: 'camera',
              type: 'custom',
              label: 'Camera',
              component: Camera
            }
          ]
        },
        {
          type: 'select',
          label: 'Microphone',
          path: 'micType',
          displayValue: true,
          options: [
            {
              label: 'OS default',
              value: 'os'
            },
            {
              label: 'BOX',
              value: 'box'
            }
          ],
          page: {
            title: 'Microphone',
            description: 'Microphone selection'
          }
        }
      ]
    },
    {
      type: 'route',
      label: 'Advanced Parameters',
      route: 'dongle',
      path: '',
      children: [
        {
          type: 'number',
          label: 'iBox Version',
          path: 'iBoxVersion',
          displayValue: true,
          page: {
            title: 'iBox Version',
            description: 'iBox Version'
          }
        },
        {
          type: 'number',
          label: 'Phone Work Mode',
          path: 'phoneWorkMode',
          displayValue: true,
          page: {
            title: 'Phone Work Mode',
            description: 'Phone Work Mode'
          }
        },
        {
          type: 'number',
          label: 'Packet Max',
          path: 'packetMax',
          displayValue: true,
          page: {
            title: 'Packet Max',
            description: 'Packet Max'
          }
        },
        {
          type: 'route',
          route: 'androidauto',
          label: 'Android Auto',
          path: '',
          children: [
            {
              type: 'number',
              label: 'DPI',
              path: 'dpi',
              displayValue: true,
              page: {
                title: 'DPI',
                description: 'DPI'
              }
            },
            {
              type: 'number',
              label: 'Format',
              path: 'format',
              displayValue: true,
              page: {
                title: 'Format',
                description: 'Format'
              }
            }
          ]
        }
      ]
    },
    {
      type: 'number',
      label: 'FFT Delay',
      path: 'visualAudioDelayMs',
      displayValue: true,
      valueTransform: {
        toView: (v: number) => v,
        fromView: (v: number) => v,
        format: (v: number) => `${v} ms`
      },
      page: {
        title: 'FFT Visualization Delay',
        description: 'Delays the FFT visualization to compensate for audio latency.'
      }
    },
    {
      type: 'route',
      label: 'Key Bindings',
      route: 'keyBindings',
      path: '',
      children: [
        { type: 'keybinding', label: 'Up', path: 'bindings', bindingKey: 'up' },
        { type: 'keybinding', label: 'Down', path: 'bindings', bindingKey: 'down' },
        { type: 'keybinding', label: 'Left', path: 'bindings', bindingKey: 'left' },
        { type: 'keybinding', label: 'Right', path: 'bindings', bindingKey: 'right' },

        { type: 'keybinding', label: 'Select Up', path: 'bindings', bindingKey: 'selectUp' },
        { type: 'keybinding', label: 'Select Down', path: 'bindings', bindingKey: 'selectDown' },

        { type: 'keybinding', label: 'Back', path: 'bindings', bindingKey: 'back' },
        { type: 'keybinding', label: 'Home', path: 'bindings', bindingKey: 'home' },

        { type: 'keybinding', label: 'Play/Pause', path: 'bindings', bindingKey: 'playOrPause' },
        { type: 'keybinding', label: 'Play', path: 'bindings', bindingKey: 'play' },
        { type: 'keybinding', label: 'Pause', path: 'bindings', bindingKey: 'pause' },

        { type: 'keybinding', label: 'Next', path: 'bindings', bindingKey: 'next' },
        { type: 'keybinding', label: 'Previous', path: 'bindings', bindingKey: 'prev' },
        { type: 'keybinding', label: 'Accept Call', path: 'bindings', bindingKey: 'acceptPhone' },
        { type: 'keybinding', label: 'Reject Call', path: 'bindings', bindingKey: 'rejectPhone' },
        { type: 'keybinding', label: 'Voice Assistant', path: 'bindings', bindingKey: 'siri' }
      ]
    },
    {
      type: 'select',
      label: 'Steering wheel position',
      path: 'hand',
      displayValue: true,
      options: [
        { label: 'LHD', value: 0 },
        { label: 'RHD', value: 1 }
      ],
      page: {
        title: 'Steering wheel position',
        description: 'Left-hand drive or right-hand drive configuration.'
      }
    },
    {
      type: 'checkbox',
      label: 'Fullscreen',
      path: 'kiosk'
    }
  ]
}
