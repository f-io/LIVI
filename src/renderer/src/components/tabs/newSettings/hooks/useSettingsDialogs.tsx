import { useCallback, useState } from 'react'
import { DialogsConfigProps, SETTINGS_DIALOGS } from '../componetns/types'
import { SettingsDialog } from '../componetns/dialog'

interface UseSettingsDialogsProps<TData extends Record<string, any>> {
  dialogsConfig: DialogsConfigProps
  data: TData
  onSave: () => void
  onChange: (key: keyof TData, value: TData[keyof TData]) => void
  onReset: () => void
}

export const useSettingsDialogs = <TData extends Record<string, any>>({
  dialogsConfig,
  data,
  onSave,
  onChange,
  onReset
}: UseSettingsDialogsProps<TData>) => {
  const [dialogState, setDialogState] = useState<Partial<Record<SETTINGS_DIALOGS, boolean>>>({})

  const activeDialog = Object.entries(dialogState).find(([_, isOpen]) => isOpen)?.[0] as
    | SETTINGS_DIALOGS
    | undefined

  const onToggleDialog = useCallback((dialog: SETTINGS_DIALOGS, state: boolean) => {
    setDialogState({ [dialog]: state })
  }, [])

  const handleClose = useCallback(() => {
    if (activeDialog) {
      onToggleDialog(activeDialog, false)
      onReset()
    }
  }, [activeDialog, onToggleDialog, onReset])

  const renderContent = () => {
    if (!activeDialog) return null
    const cfg = dialogsConfig[activeDialog]
    if (!cfg) return null
    return cfg.content({ data, onChange } as any)
  }

  return {
    dialog: () =>
      activeDialog ? (
        <SettingsDialog
          isOpen
          title={dialogsConfig[activeDialog]?.title}
          onClose={handleClose}
          type={activeDialog}
          actions={[
            {
              title: 'Save',
              handler: onSave
            }
          ]}
        >
          {renderContent()}
        </SettingsDialog>
      ) : null,
    onToggleDialog
  }
}
