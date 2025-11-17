import { ExtraConfig } from '@main/Globals'
import { DefaultDialogProps } from '../dialog/types'

export type ScreenResolutionProps = DefaultDialogProps<
  VideoSettingKey,
  { width: number; height: number }
>

export type FpsProps = DefaultDialogProps<'fps', { fps: number }>

export type MediaDelayProps = DefaultDialogProps<'mediaDelay', { mediaDelay: number }>

export type AudioVolumeProps = DefaultDialogProps<'audioVolume', { audioVolume: number }>

export type NavVolumeProps = DefaultDialogProps<'navVolume', { navVolume: number }>

export type VideoSettingKey = keyof Pick<
  ExtraConfig,
  'width' | 'height' | 'fps' | 'mediaDelay' | 'audioVolume' | 'navVolume'
>
