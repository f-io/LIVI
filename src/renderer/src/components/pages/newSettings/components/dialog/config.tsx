import { ScreenResolution } from '../../subPages/videoAudio/dialogs/ScreenResolution'
import { Fps } from '../../subPages/videoAudio/dialogs/Fps'
import { MediaDelay } from '../../subPages/videoAudio/dialogs/MediaDelay'
import { AudioVolume } from '../../subPages/videoAudio/dialogs/AudioVolume'
import { NavVolume } from '../../subPages/videoAudio/dialogs/NavVolume'
import { CarName } from '../../subPages/general/dialogs/CarName'
import { OemName } from '../../subPages/general/dialogs/OemName'
import { Dpi } from '../../subPages/other/dialogs/Dpi'
import { Format } from '../../subPages/other/dialogs/Format'
import { DialogsConfigProps, SETTINGS_DIALOGS } from '../../subPages/types'
import { ScreenResolutionProps } from '../../subPages/videoAudio/types'

export const dialogsConfig: DialogsConfigProps = {
  [SETTINGS_DIALOGS.SCREEN_RESOLUTION]: {
    title: 'Stream resolution',
    content: (props: ScreenResolutionProps) => <ScreenResolution {...props} />
  },
  [SETTINGS_DIALOGS.FPS]: {
    title: 'PS',
    content: (props) => <Fps {...props} />
  },
  [SETTINGS_DIALOGS.MEDIA_DELAY]: {
    title: 'Media Delay',
    content: (props) => <MediaDelay {...props} />
  },
  [SETTINGS_DIALOGS.AUDIO_VOLUME]: {
    title: 'Audio Volume',
    content: (props) => <AudioVolume {...props} />
  },
  [SETTINGS_DIALOGS.NAV_VOLUME]: {
    title: 'Navigation Volume',
    content: (props) => <NavVolume {...props} />
  },
  [SETTINGS_DIALOGS.CAR_NAME]: {
    title: 'Car Name',
    content: (props) => <CarName {...props} />
  },
  [SETTINGS_DIALOGS.OEM_NAME]: {
    title: 'UI Label Name',
    content: (props) => <OemName {...props} />
  },
  [SETTINGS_DIALOGS.DPI]: {
    title: 'DPI',
    content: (props) => <Dpi {...props} />
  },
  [SETTINGS_DIALOGS.FORMAT]: {
    title: 'Format',
    content: (props) => <Format {...props} />
  }
}
