import { useCarplayStore } from '@store/store'
import { SettingsLayout } from '@renderer/components/layouts'
import { EMPTY_STRING } from '@renderer/constants'
import { Typography } from '@mui/material'
import Select from '@mui/material/Select'
import MenuItem from '@mui/material/MenuItem'
import { StackItem } from '../../StackItem'
import { useSmartSettings } from '../../../hooks/useSmartSettings'
import { advancedSettingsUIConfig } from '../config'
import { AdvancedSettingKey } from '../types'
import { dialogsConfig } from '../../../components/dialog/config'
import { useSettingsDialogs } from '../../../hooks'

import { get } from 'lodash'

export const Advanced = () => {
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<AdvancedSettingKey, number> = {
    dpi: settings?.dpi ?? '',
    format: settings?.format ?? '',
    language: settings?.language ?? 'en'
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

  console.log(settingsState)

  return (
    <SettingsLayout onSave={save}>
      {(Object.keys(advancedSettingsUIConfig) as AdvancedSettingKey[])
        .filter((key) => key !== 'language')
        .map((key) => {
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
              ) : null}
            </StackItem>
          )
        })}

      <StackItem>
        <Typography>{advancedSettingsUIConfig['language'].label}</Typography>

        <Select
          labelId="select-camera"
          id="select-camera"
          size="small"
          value={settingsState['language']}
          sx={{
            minWidth: '200px'
          }}
          onChange={(e) => handleFieldChange('language', e.target.value)}
        >
          <MenuItem value={'en'}>English</MenuItem>
        </Select>
      </StackItem>
      {dialog && dialog()}
    </SettingsLayout>
  )
}
