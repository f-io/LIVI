import { SETTINGS_DIALOGS } from '../types'

export interface DialogProps {
  children: React.ReactNode
  type: SETTINGS_DIALOGS | string
  title: string
  isOpen: boolean
  onClose: (dialog: SETTINGS_DIALOGS, state: boolean) => void
  actions: {
    title: string
    icon?: string
    handler: () => void
  }[]
}

export interface DefaultDialogProps<
  FieldKey extends string | number | symbol,
  DataShape extends Record<string, any>
> {
  isOpen: boolean
  data: DataShape
  onClose: (dialog: SETTINGS_DIALOGS, status: boolean) => void
  onChange: (field: FieldKey, value: number | string | boolean) => void
  onSave: () => void
}
