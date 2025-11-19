import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { useCarplayStore } from '@renderer/store/store'
import { Button, Typography, useTheme, TextField, InputAdornment } from '@mui/material'
import { StackItem } from '../../StackItem'
import { THEME } from '@renderer/constants'
import { useSmartSettings } from '../../../hooks/useSmartSettings'
import { uiThemeSettingsUIConfig } from '../config'
import { UiThemeSettingKey } from '../types'

export const UiTheme = () => {
  const settings = useCarplayStore((s) => s.settings)
  const theme = useTheme()

  const isDarkMode = theme.palette.mode === THEME.DARK

  const currentPrimary =
    (isDarkMode ? settings.primaryColorDark : settings.primaryColorLight) ??
    theme.palette.primary.main

  const initialState: Record<UiThemeSettingKey, number> = {
    primaryColor: settings?.primaryColor ?? currentPrimary,
    highlightEditableField: settings?.highlightEditableField ?? currentPrimary
  }

  const {
    state: settingsState,
    handleFieldChange,
    save,
    resetState
  } = useSmartSettings(initialState, settings)

  return (
    <SettingsLayout onSave={save}>
      {(Object.keys(uiThemeSettingsUIConfig) as UiThemeSettingKey[]).map((key) => {
        const cfg = uiThemeSettingsUIConfig[key]

        return (
          <StackItem key={key}>
            <Typography>{cfg.label}</Typography>

            <TextField
              size="small"
              label={isDarkMode ? 'PRIMARY (DARK)' : 'PRIMARY (LIGHT)'}
              type="color"
              value={settingsState[key]}
              fullWidth
              onChange={(e) => {
                handleFieldChange(key, e.target.value)
              }}
              slotProps={{
                inputLabel: { shrink: true },
                input: {
                  endAdornment: (
                    <InputAdornment position="end">
                      <Button
                        size="small"
                        variant="outlined"
                        onClick={() => {
                          resetState()
                        }}
                        sx={{ ml: 1, py: 0.25, px: 1 }}
                      >
                        RESET
                      </Button>
                    </InputAdornment>
                  )
                }
              }}
            />
          </StackItem>
        )
      })}
    </SettingsLayout>
  )
}
