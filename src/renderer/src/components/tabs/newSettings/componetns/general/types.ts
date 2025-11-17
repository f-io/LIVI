import { ExtraConfig } from '@main/Globals'
import { DefaultDialogProps } from '../dialog/types'

export type GeneralSettingKey = keyof Pick<
  ExtraConfig,
  'autoPlay' | 'audioTransferMode' | 'nightMode' | 'kiosk' | 'carName' | 'oemName'
>

export type CarNameProps = DefaultDialogProps<'carName', { carName: string }>

export type OemNameProps = DefaultDialogProps<'oemName', { oemName: string }>
