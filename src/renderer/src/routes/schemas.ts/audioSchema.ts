import { SettingsNode, ValueTransform } from '../types'
import { ExtraConfig } from '../../../../main/Globals'

const audioValueTransform: ValueTransform<number | undefined, number> = {
  toView: (v) => Math.round((v ?? 1) * 100),
  fromView: (v, prev) => {
    const next = v / 100
    if (!Number.isFinite(next)) return prev ?? 1
    return next
  },
  format: (v) => `${v} %`
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
        description: 'Music Volume'
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
        description: 'Navigation volume'
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
        description: 'Siri volume'
      }
    },
    {
      type: 'slider',
      label: 'Phone Call',
      path: 'callVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Phone Call',
        description: 'Phone call volume'
      }
    },
    {
      type: 'number',
      label: 'Audio Buffer',
      path: 'mediaDelay',
      step: 50,
      min: 300,
      max: 2000,
      default: 1000,
      displayValue: true,
      displayValueUnit: 'ms',
      valueTransform: {
        toView: (v: number | undefined) => v ?? 1000,
        fromView: (v: number, prev?: number) => {
          const next = Math.round(v / 50) * 50
          if (!Number.isFinite(next)) return prev ?? 1000
          return next
        },
        format: (v: number) => `${v} ms`
      },
      page: {
        title: 'Audio Buffer',
        description: 'Dongle audio buffer size in ms'
      }
    },
    {
      type: 'select',
      label: 'Sampling Frequency',
      path: 'mediaSound',
      displayValue: true,
      options: [
        { label: '44.1 kHz', value: 0 },
        { label: '48 kHz', value: 1 }
      ],
      page: {
        title: 'Sampling Frequency',
        description: 'Native stream sampling frequency'
      }
    },
    {
      type: 'select',
      label: 'Call Quality',
      path: 'callQuality',
      displayValue: true,
      options: [
        { label: 'Low', value: 0 },
        { label: 'Medium', value: 1 },
        { label: 'High', value: 2 }
      ],
      page: {
        title: 'Call Quality',
        description: 'Call quality, will affect bandwidth usage'
      }
    },
    {
      type: 'checkbox',
      label: 'Disable Audio',
      path: 'audioTransferMode'
    }
  ]
}
