import type { Config } from '@shared/types'
import { SelectOption, SettingsNode, ValueTransform } from '../types'

const audioValueTransform: ValueTransform<number | undefined, number> = {
  toView: (v) => Math.round((v ?? 1) * 100),
  fromView: (v, prev) => {
    const next = v / 100
    if (!Number.isFinite(next)) return prev ?? 1
    return next
  },
  format: (v) => `${v} %`
}

const systemDefaultOption: SelectOption = {
  value: '',
  label: 'System default',
  labelKey: 'settings.audioDeviceSystemDefault'
}

async function loadAudioOutputDevices(): Promise<SelectOption[]> {
  const api = window.projection?.audio
  if (!api?.listSinks) return [systemDefaultOption]
  const list = await api.listSinks()
  return [
    systemDefaultOption,
    ...list.map((d) => ({ value: d.id, label: d.name, offline: d.offline }))
  ]
}

async function loadAudioInputDevices(): Promise<SelectOption[]> {
  const api = window.projection?.audio
  if (!api?.listSources) return [systemDefaultOption]
  const list = await api.listSources()
  return [
    systemDefaultOption,
    ...list.map((d) => ({ value: d.id, label: d.name, offline: d.offline }))
  ]
}

export const audioSchema: SettingsNode<Config> = {
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
        labelTitle: 'settings.music'
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
        labelTitle: 'settings.navigation'
      }
    },
    {
      type: 'slider',
      label: 'Voice Assistant',
      labelKey: 'settings.voiceAssistant',
      path: 'voiceAssistantVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Voice Assistant',
        labelTitle: 'settings.voiceAssistant'
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
        labelTitle: 'settings.phoneCalls'
      }
    },
    {
      type: 'slider',
      label: 'System Sounds',
      labelKey: 'settings.systemSounds',
      path: 'systemSoundsVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'System Sounds',
        labelTitle: 'settings.systemSounds'
      }
    },
    {
      type: 'select',
      label: 'Audio Output',
      labelKey: 'settings.audioOutputDevice',
      path: 'audioOutputDevice',
      labelPath: 'audioOutputDeviceLabel',
      displayValue: true,
      options: [systemDefaultOption],
      loadOptions: loadAudioOutputDevices,
      page: {
        title: 'Audio Output',
        labelTitle: 'settings.audioOutputDevice'
      }
    },
    {
      type: 'select',
      label: 'Audio Input',
      labelKey: 'settings.audioInputDevice',
      path: 'audioInputDevice',
      labelPath: 'audioInputDeviceLabel',
      displayValue: true,
      options: [systemDefaultOption],
      loadOptions: loadAudioInputDevices,
      page: {
        title: 'Audio Input',
        labelTitle: 'settings.audioInputDevice'
      }
    },
    {
      type: 'select',
      label: 'Sampling Frequency',
      labelKey: 'settings.samplingFrequency',
      path: 'samplingFrequency',
      displayValue: true,
      options: [
        { label: '44.1 kHz', value: 0 },
        { label: '48 kHz', value: 1 }
      ],
      page: {
        title: 'Sampling Frequency',
        labelTitle: 'settings.samplingFrequency'
      }
    },
    {
      type: 'checkbox',
      label: 'Disable Audio',
      labelKey: 'settings.disableAudio',
      path: 'disableAudioOutput'
    }
  ]
}
