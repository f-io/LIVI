import { ScreenResolution } from '../videoAudio/dialogs/ScreenResolution'
import { Fps } from '../videoAudio/dialogs/Fps'
import { MediaDelay } from '../videoAudio/dialogs/MediaDelay'
import { AudioVolume } from '../videoAudio/dialogs/AudioVolume'
import { NavVolume } from '../videoAudio/dialogs/NavVolume'
import { CarName } from '../general/dialogs/CarName'
import { OemName } from '../general/dialogs/OemName'
import { DialogsConfigProps, SETTINGS_DIALOGS } from '../types'
import { ScreenResolutionProps } from '../videoAudio/types'

export const dialogsConfig: DialogsConfigProps = {
  [SETTINGS_DIALOGS.SCREEN_RESOLUTION]: {
    title: 'Set Screen Size',
    content: (props: ScreenResolutionProps) => <ScreenResolution {...props} />
  },
  [SETTINGS_DIALOGS.FPS]: {
    title: 'Set FPS',
    content: (props) => <Fps {...props} />
  },
  [SETTINGS_DIALOGS.MEDIA_DELAY]: {
    title: 'Set Media Delay',
    content: (props) => <MediaDelay {...props} />
  },
  [SETTINGS_DIALOGS.AUDIO_VOLUME]: {
    title: 'Set Audio Volume',
    content: (props) => <AudioVolume {...props} />
  },
  [SETTINGS_DIALOGS.NAV_VOLUME]: {
    title: 'Set Navigation Volume',
    content: (props) => <NavVolume {...props} />
  },
  [SETTINGS_DIALOGS.CAR_NAME]: {
    title: 'Set Car Name',
    content: (props) => <CarName {...props} />
  },
  [SETTINGS_DIALOGS.OEM_NAME]: {
    title: 'Set Car Name',
    content: (props) => <OemName {...props} />
  }
}
