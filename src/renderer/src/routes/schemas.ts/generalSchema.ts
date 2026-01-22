import { SettingsNode } from '../types'
import { ExtraConfig } from '../../../../main/Globals'

export const generalSchema: SettingsNode<ExtraConfig> = {
  route: 'general',
  label: 'General',
  labelKey: 'settings.general',
  type: 'route',
  path: '',
  children: [
    {
      type: 'route',
      route: 'connections',
      label: 'Connections',
      labelKey: 'settings.connections',
      path: '',
      children: [
        {
          type: 'string',
          label: 'Car Name',
          labelKey: 'settings.carName',
          path: 'carName',
          displayValue: true,
          page: {
            title: 'Car Name',
            labelTitle: 'settings.carName',
            description: 'The name of the CarPlay device',
            labelDescription: 'settings.carNameDescription'
          }
        },
        {
          type: 'string',
          label: 'UI Name',
          labelKey: 'settings.uiName',
          path: 'oemName',
          displayValue: true,
          page: {
            title: 'UI Name',
            labelTitle: 'settings.uiName',
            description: 'The name displayed in the CarPlay UI.',
            labelDescription: 'settings.uiNameDescription'
          }
        },
        {
          type: 'checkbox',
          label: 'Auto Connect',
          labelKey: 'settings.autoConnect',
          path: 'autoConn'
        },
        {
          type: 'route',
          route: 'wifi',
          label: 'Wi-Fi',
          labelKey: 'settings.wifi',
          path: '',
          children: [
            {
              type: 'select',
              label: 'Wi-Fi Frequency',
              labelKey: 'settings.wifiFrequency',
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
                labelTitle: 'settings.wifiFrequency',
                description: 'Wi-Fi frequency selection',
                labelDescription: 'settings.wifiFrequencyDescription'
              }
            }
          ]
        }
      ]
    },
    {
      type: 'route',
      label: 'Advanced Parameters',
      labelKey: 'settings.advancedParameters',
      route: 'dongle',
      path: '',
      children: [
        {
          type: 'route',
          route: 'androidauto',
          label: 'Android Auto',
          labelKey: 'settings.androidAuto',
          path: '',
          children: [
            {
              type: 'number',
              label: 'DPI',
              labelKey: 'settings.dpi',
              path: 'dpi',
              displayValue: true,
              page: {
                title: 'DPI',
                labelTitle: 'settings.dpi',
                description: 'Set the DPI (Dots Per Inch) for the display',
                labelDescription: 'settings.dpiDescription'
              }
            },
            {
              type: 'number',
              label: 'Format',
              labelKey: 'settings.format',
              path: 'format',
              displayValue: true,
              page: {
                title: 'Format',
                labelTitle: 'settings.format',
                description: 'Format',
                labelDescription: 'settings.formatDescription'
              }
            }
          ]
        }
      ]
    },
    {
      type: 'number',
      label: 'FFT Delay',
      labelKey: 'settings.fftDelay',
      path: 'visualAudioDelayMs',
      displayValue: true,
      valueTransform: {
        toView: (v: number) => v,
        fromView: (v: number) => v,
        format: (v: number) => `${v} ms`
      },
      page: {
        title: 'FFT Visualization Delay',
        labelTitle: 'settings.fftDelay',
        description: 'Delays the FFT visualization to compensate for audio latency.',
        labelDescription: 'settings.fftDelayDescription'
      }
    },
    {
      type: 'route',
      label: 'Key Bindings',
      labelKey: 'settings.keyBindings',
      route: 'keyBindings',
      path: '',
      children: [
        {
          type: 'keybinding',
          label: 'Up',
          labelKey: 'settings.up',
          path: 'bindings',
          bindingKey: 'up'
        },
        {
          type: 'keybinding',
          label: 'Down',
          labelKey: 'settings.down',
          path: 'bindings',
          bindingKey: 'down'
        },
        {
          type: 'keybinding',
          label: 'Left',
          labelKey: 'settings.left',
          path: 'bindings',
          bindingKey: 'left'
        },
        {
          type: 'keybinding',
          label: 'Right',
          labelKey: 'settings.right',
          path: 'bindings',
          bindingKey: 'right'
        },

        {
          type: 'keybinding',
          label: 'Select Up',
          labelKey: 'settings.selectUp',
          path: 'bindings',
          bindingKey: 'selectUp'
        },
        {
          type: 'keybinding',
          label: 'Select Down',
          labelKey: 'settings.selectDown',
          path: 'bindings',
          bindingKey: 'selectDown'
        },

        {
          type: 'keybinding',
          label: 'Back',
          labelKey: 'settings.back',
          path: 'bindings',
          bindingKey: 'back'
        },
        {
          type: 'keybinding',
          label: 'Home',
          labelKey: 'settings.home',
          path: 'bindings',
          bindingKey: 'home'
        },

        {
          type: 'keybinding',
          label: 'Play/Pause',
          labelKey: 'settings.playPause',
          path: 'bindings',
          bindingKey: 'playPause'
        },
        {
          type: 'keybinding',
          label: 'Play',
          labelKey: 'settings.play',
          path: 'bindings',
          bindingKey: 'play'
        },
        {
          type: 'keybinding',
          label: 'Pause',
          labelKey: 'settings.pause',
          path: 'bindings',
          bindingKey: 'pause'
        },

        {
          type: 'keybinding',
          label: 'Next',
          labelKey: 'settings.next',
          path: 'bindings',
          bindingKey: 'next'
        },
        {
          type: 'keybinding',
          label: 'Previous',
          labelKey: 'settings.previous',
          path: 'bindings',
          bindingKey: 'prev'
        },
        {
          type: 'keybinding',
          label: 'Accept Call',
          labelKey: 'settings.acceptCall',
          path: 'bindings',
          bindingKey: 'acceptPhone'
        },
        {
          type: 'keybinding',
          label: 'Reject Call',
          labelKey: 'settings.rejectCall',
          path: 'bindings',
          bindingKey: 'rejectPhone'
        },
        {
          type: 'keybinding',
          label: 'Voice Assistant',
          labelKey: 'settings.voiceAssistant',
          path: 'bindings',
          bindingKey: 'siri'
        }
      ]
    },
    {
      type: 'select',
      label: 'Steering wheel position',
      labelKey: 'settings.steeringWheelPosition',
      path: 'hand',
      displayValue: true,
      options: [
        { label: 'LHD', labelKey: 'settings.lhdr', value: 0 },
        { label: 'RHD', labelKey: 'settings.rhdr', value: 1 }
      ],
      page: {
        title: 'Steering wheel position',
        labelTitle: 'settings.steeringWheelPosition',
        description: 'Set the position of the steering wheel controls.',
        labelDescription: 'settings.steeringWheelPositionDescription'
      }
    },
    {
      type: 'select',
      label: 'Language',
      labelKey: 'settings.language',
      path: 'language',
      displayValue: true,
      options: [
        { label: 'English', labelKey: 'settings.english', value: 'en' },
        { label: 'German', labelKey: 'settings.german', value: 'de' },
        { label: 'Ukrainian', labelKey: 'settings.ukrainian', value: 'ua' }
      ],
      page: {
        title: 'Language',
        labelTitle: 'settings.language',
        description: 'Select the application language',
        labelDescription: 'settings.languageDescription'
      }
    },
    {
      type: 'checkbox',
      label: 'Fullscreen',
      labelKey: 'settings.fullscreen',
      path: 'kiosk'
    }
  ]
}
