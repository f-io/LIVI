import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { StackItem } from '../StackItem'
import { Typography, Switch } from '@mui/material'
import { useCarplayStore } from '@store/store'
import { GeneralSettingKey } from './types'
import { generalSettingsUIConfig } from './config'
import { useSettingsDialogs } from '../../hooks'
import { dialogsConfig } from '../../components/dialog/config'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { get } from 'lodash'
import { EMPTY_STRING } from '@renderer/constants'

export const General = () => {
  // FIXME types
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<GeneralSettingKey, number | string | boolean> = {
    carName: settings?.carName || '',
    oemName: settings?.oemName || '',
    autoPlay: settings?.autoPlay,
    audioTransferMode: settings?.audioTransferMode,
    nightMode: settings?.nightMode,
    kiosk: settings?.kiosk
  }

  const {
    state: settingsState,
    handleFieldChange,
    resetState,
    save
  } = useSmartSettings(initialState, settings, {
    overrides: {
      audioTransferMode: {
        transform: (v: boolean) => !v
      }
    }
  })

  const { dialog, onToggleDialog } = useSettingsDialogs({
    dialogsConfig,
    data: settingsState,
    onSave: save,
    onChange: handleFieldChange,
    onReset: resetState
  })

  return (
    <SettingsLayout onSave={save}>
      {(Object.keys(generalSettingsUIConfig) as GeneralSettingKey[]).map((key) => {
        const cfg = generalSettingsUIConfig[key]
        const _onClick = () =>
          cfg.dialog
            ? onToggleDialog(cfg.dialog!, true)
            : handleFieldChange(key, !settingsState[key])

        return (
          <StackItem key={key} onClick={_onClick}>
            <Typography>{generalSettingsUIConfig[key].label}</Typography>

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
