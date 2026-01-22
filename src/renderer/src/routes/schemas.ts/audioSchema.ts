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
  labelKey: 'settings.audio',
  path: '',
  children: [
    {
      type: 'slider',
      label: 'Music',
      labelKey: 'settings.music',
      path: 'audioVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Music',
        labelTitle: 'settings.music',
        description: 'Music volume',
        labelDescription: 'settings.musicDescription'
      }
    },
    {
      type: 'slider',
      label: 'Navigation',
      labelKey: 'settings.navigation',
      path: 'navVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Navigation',
        labelTitle: 'settings.navigation',
        description: 'Navigation volume',
        labelDescription: 'settings.navigationDescription'
      }
    },
    {
      type: 'slider',
      label: 'Siri',
      labelKey: 'settings.siri',
      path: 'siriVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Siri',
        labelTitle: 'settings.siri',
        description: 'Siri voice assistant settings',
        labelDescription: 'settings.siriDescription'
      }
    },
    {
      type: 'slider',
      label: 'Phone Calls',
      labelKey: 'settings.phoneCalls',
      path: 'callVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Phone Calls',
        labelTitle: 'settings.phoneCalls',
        description: 'Phone call volume',
        labelDescription: 'settings.phoneCallsDescription'
      }
    },
    {
      type: 'number',
      label: 'Audio Buffer',
      labelKey: 'settings.audioBufferSize',
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
        labelTitle: 'settings.audioBufferSize',
        description: 'Dongle audio buffer size in ms',
        labelDescription: 'settings.audioBufferDescription'
      }
    },
    {
      type: 'select',
      label: 'Sampling Frequency',
      labelKey: 'settings.samplingFrequency',
      path: 'mediaSound',
      displayValue: true,
      options: [
        { label: '44.1 kHz', value: 0 },
        { label: '48 kHz', value: 1 }
      ],
      page: {
        title: 'Sampling Frequency',
        labelTitle: 'settings.samplingFrequency',
        description: 'Native stream sampling frequency',
        labelDescription: 'settings.samplingFrequencyDescription'
      }
    },
    {
      type: 'select',
      label: 'Call Quality',
      labelKey: 'settings.callQuality',
      path: 'callQuality',
      displayValue: true,
      options: [
        { label: 'Low', labelKey: 'settings.callQualityLow', value: 0 },
        { label: 'Medium', labelKey: 'settings.callQualityMedium', value: 1 },
        { label: 'High', labelKey: 'settings.callQualityHigh', value: 2 }
      ],
      page: {
        title: 'Call Quality',
        labelTitle: 'settings.callQuality',
        description: 'Call quality, will affect bandwidth usage',
        labelDescription: 'settings.callQualityDescription'
      }
    },
    {
      type: 'select',
      label: 'Microphone',
      labelKey: 'settings.microphone',
      path: 'micType',
      displayValue: true,
      options: [
        {
          label: 'OS default',
          labelKey: 'settings.osDefault',
          value: 'os'
        },
        {
          label: 'BOX',
          labelKey: 'settings.box',
          value: 'box'
        }
      ],
      page: {
        title: 'Microphone',
        labelTitle: 'settings.microphone',
        description: 'Microphone selection',
        labelDescription: 'settings.microphoneDescription'
      }
    },
    {
      type: 'checkbox',
      label: 'Disable Audio',
      labelKey: 'settings.disableAudio',
      path: 'audioTransferMode'
    }
  ]
}
