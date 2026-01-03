import { SettingsNode } from '../types'
import { ExtraConfig } from '../../../../main/Globals'

const audioValueTransform = {
  toView: (v: number) => Math.round(v * 100),
  fromView: (v: number) => v / 100,
  format: (v: number) => `${v} %`
}

export const audioSchema: SettingsNode<ExtraConfig> = {
  type: 'route',
  route: 'audio',
  label: 'Audio',
  path: '',
  children: [
    {
      type: 'slider',
      label: 'Music',
      path: 'audioVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Music',
        description: 'Music'
      }
    },
    {
      type: 'slider',
      label: 'Navigation',
      path: 'navVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Navigation',
        description: 'Navigation'
      }
    },
    {
      type: 'slider',
      label: 'Siri',
      path: 'siriVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Siri',
        description: 'Siri'
      }
    },
    {
      type: 'slider',
      label: 'Phone call',
      path: 'callVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Phone call',
        description: 'Phone call'
      }
    },
    {
      type: 'number',
      label: 'Audio Buffer',
      path: 'mediaDelay',
      step: 50, // to-do implement step for number type
      min: 300, // to-do implement min/max for number type
      max: 2000,
      default: 1000, // to-do implement default for number type
      displayValue: true,
      displayValueUnit: 'ms',
      valueTransform: {
        toView: (v: number) => v,
        fromView: (v: number) => Math.round(v / 50) * 50,
        format: (v: number) => `${v} ms`
      },
      page: {
        title: 'Audio buffer',
        description: 'Dongle internal audio buffer size in milliseconds'
      }
    },
    {
      type: 'select',
      label: 'Sampling Frequency',
      path: 'mediaSound',
      displayValue: true,
      options: [
        {
          label: '44.1 kHz',
          value: 0
        },
        {
          label: '48 kHz',
          value: 1
        }
      ],
      page: {
        title: 'Sampling Frequency',
        description: 'Sampling Frequency'
      }
    },
    {
      type: 'select',
      label: 'Call Quality',
      path: 'callQuality',
      displayValue: true,
      options: [
        {
          label: 'Low',
          value: 0
        },
        {
          label: 'Medium',
          value: 1
        },
        {
          label: 'High',
          value: 2
        }
      ],
      page: {
        title: 'Call Quality',
        description: 'Call Quality'
      }
    },
    {
      type: 'checkbox',
      label: 'Disable CarPlay Audio',
      path: 'audioTransferMode'
    }
  ]
}
