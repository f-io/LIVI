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
  return [systemDefaultOption, ...list.map((d) => ({ value: d.id, label: d.name }))]
}

async function loadAudioInputDevices(): Promise<SelectOption[]> {
  const api = window.projection?.audio
  if (!api?.listSources) return [systemDefaultOption]
  const list = await api.listSources()
  return [systemDefaultOption, ...list.map((d) => ({ value: d.id, label: d.name }))]
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
      label: 'Voice Assistant',
      labelKey: 'settings.voiceAssistant',
      path: 'voiceAssistantVolume',
      displayValue: true,
      displayValueUnit: '%',
      valueTransform: audioValueTransform,
      page: {
        title: 'Voice Assistant',
        labelTitle: 'settings.voiceAssistant',
        description: 'Voice assistant volume',
        labelDescription: 'settings.voiceAssistantDescription'
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
      type: 'select',
      label: 'Audio Output',
      labelKey: 'settings.audioOutputDevice',
      path: 'audioOutputDevice',
      displayValue: true,
      options: [systemDefaultOption],
      loadOptions: loadAudioOutputDevices,
      page: {
        title: 'Audio Output',
        labelTitle: 'settings.audioOutputDevice',
        description: 'Pick the audio sink.',
        labelDescription: 'settings.audioOutputDeviceDescription'
      }
    },
    {
      type: 'select',
      label: 'Audio Input',
      labelKey: 'settings.audioInputDevice',
      path: 'audioInputDevice',
      displayValue: true,
      options: [systemDefaultOption],
      loadOptions: loadAudioInputDevices,
      page: {
        title: 'Audio Input',
        labelTitle: 'settings.audioInputDevice',
        description: 'Pick the microphone source.',
        labelDescription: 'settings.audioInputDeviceDescription'
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
        labelTitle: 'settings.samplingFrequency',
        description: 'Phone main-audio stream sampling frequency.',
        labelDescription: 'settings.samplingFrequencyDescription'
      }
    },
    // Currently disabled
    /*
    {
      type: 'checkbox',
      label: 'Play on Connect',
      labelKey: 'settings.playOnConnect',
      path: 'autoPlay'
    },*/
    {
      type: 'checkbox',
      label: 'Disable Audio',
      labelKey: 'settings.disableAudio',
      path: 'disableAudioOutput'
    }
  ]
}
