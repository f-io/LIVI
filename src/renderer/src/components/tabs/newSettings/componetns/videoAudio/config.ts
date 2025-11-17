import { VideoSettingKey } from './types'
import { SETTINGS_DIALOGS } from '../types'

export const videoSettingsUIConfig: Record<
  VideoSettingKey,
  {
    label: string
    dialog?: SETTINGS_DIALOGS
    display?: (state: Record<VideoSettingKey, number>) => string | number
  }
> = {
  width: {
    label: 'Screen Size',
    dialog: SETTINGS_DIALOGS.SCREEN_RESOLUTION,
    display: (state) => `${state.width} x ${state.height}`
  },
  height: {
    label: 'Screen Size'
  },
  fps: {
    label: 'FPS',
    dialog: SETTINGS_DIALOGS.FPS,
    display: (state) => state.fps
  },
  mediaDelay: {
    label: 'Media Delay',
    dialog: SETTINGS_DIALOGS.MEDIA_DELAY,
    display: (state) => state.mediaDelay
  },
  audioVolume: {
    label: 'Audio Volume',
    dialog: SETTINGS_DIALOGS.AUDIO_VOLUME,
    display: (state) => Math.round((state.audioVolume || 0) * 100)
  },
  navVolume: {
    label: 'Navigation Volume',
    dialog: SETTINGS_DIALOGS.NAV_VOLUME,
    display: (state) => Math.round((state.navVolume || 0) * 100)
  }
}
