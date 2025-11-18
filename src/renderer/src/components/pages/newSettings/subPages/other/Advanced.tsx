import { useCarplayStore } from '@store/store'
import { SettingsLayout } from '@renderer/components/layouts'
import { EMPTY_STRING } from '@renderer/constants'
import { Typography, Switch } from '@mui/material'
import { StackItem } from '../StackItem'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { advancedSettingsUIConfig } from './config'
import { AdvancedSettingKey } from './types'
import { dialogsConfig } from '../../components/dialog/config'
import { useSettingsDialogs } from '../../hooks'

import { get } from 'lodash'

export const Advanced = () => {
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<AdvancedSettingKey, number> = {
    dpi: settings?.dpi ?? '',
    format: settings?.format ?? ''
  }

  const {
    state: settingsState,
    handleFieldChange,
    resetState,
    save
  } = useSmartSettings(initialState, settings)

  const { dialog, onToggleDialog } = useSettingsDialogs({
    dialogsConfig,
    data: settingsState,
    onSave: save,
    onChange: handleFieldChange,
    onReset: resetState
  })

  return (
    <SettingsLayout onSave={save}>
      {(Object.keys(advancedSettingsUIConfig) as AdvancedSettingKey[]).map((key) => {
        const cfg = advancedSettingsUIConfig[key]
        const _onClick = () =>
          cfg.dialog
            ? onToggleDialog(cfg.dialog!, true)
            : handleFieldChange(key, !settingsState[key])

        return (
          <StackItem key={key} onClick={_onClick}>
            <Typography>{advancedSettingsUIConfig[key].label}</Typography>

            {cfg.dialog ? (
              <Typography style={{ justifyContent: 'flex-end' }}>
                {cfg.display
                  ? cfg.display(settingsState)
                  : get(settingsState[key], '', EMPTY_STRING)}
              </Typography>
            ) : (
              <Switch
                checked={!!settingsState[key]}
                onChange={(e) => handleFieldChange(key, e.target.checked)}
                slotProps={{ input: { 'aria-label': key } }}
              />
            )}
          </StackItem>
        )
      })}
      {dialog && dialog()}
    </SettingsLayout>
  )
}
