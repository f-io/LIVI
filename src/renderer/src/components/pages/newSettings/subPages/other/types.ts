import { ExtraConfig } from '@main/Globals'
import { DefaultDialogProps } from '../../components/dialog/types'

export type AdvancedSettingKey = keyof Pick<ExtraConfig, 'dpi' | 'format' | 'language'>

export type UiThemeSettingKey = keyof Pick<ExtraConfig, 'primaryColor' | 'highlightEditableField'>

export type DpiProps = DefaultDialogProps<'dpi', { dpi: number }>

export type FormatProps = DefaultDialogProps<'format', { format: number }>
