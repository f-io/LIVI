import { ReactNode } from 'react'
import {
  FpsProps,
  ScreenResolutionProps,
  MediaDelayProps,
  AudioVolumeProps,
  NavVolumeProps
} from './videoAudio/types'
import { DpiProps, FormatProps } from './other/types'

export enum SETTINGS_DIALOGS {
  SCREEN_RESOLUTION = 'SCREEN_RESOLUTION',
  FPS = 'FPS',
  MEDIA_DELAY = 'MEDIA_DELAY',
  AUDIO_VOLUME = 'AUDIO_VOLUME',
  NAV_VOLUME = 'NAV_VOLUME',
  DPI = 'DPI',
  FORMAT = 'FORMAT'
}

export interface StackItemProps {
  children?: ReactNode
  withForwardIcon?: boolean
  value?: string
  onClick?: () => void
}

export type DialogPropsMap = {
  [SETTINGS_DIALOGS.SCREEN_RESOLUTION]: ScreenResolutionProps
  [SETTINGS_DIALOGS.FPS]: FpsProps
  [SETTINGS_DIALOGS.MEDIA_DELAY]: MediaDelayProps
  [SETTINGS_DIALOGS.AUDIO_VOLUME]: AudioVolumeProps
  [SETTINGS_DIALOGS.NAV_VOLUME]: NavVolumeProps
  [SETTINGS_DIALOGS.DPI]: DpiProps
  [SETTINGS_DIALOGS.FORMAT]: FormatProps
}

export type DialogsConfigProps = {
  [K in keyof DialogPropsMap]: {
    title: string
    content: (props: DialogPropsMap[K]) => React.ReactNode
  }
}
