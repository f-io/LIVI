import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { StackItem } from '../StackItem'
import { Typography } from '@mui/material'
import ToggleButton from '@mui/material/ToggleButton'
import ToggleButtonGroup from '@mui/material/ToggleButtonGroup'
import Select from '@mui/material/Select'
import MenuItem from '@mui/material/MenuItem'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { useCarplayStore } from '@store/store'
import { SourcesSettingKey } from './types'
import { sourcesSettingsUIConfig } from './config'
import { useMemo } from 'react'
import { useCameraDetection } from './useCameraDetection'
import { coerceSelectValue } from './utils'

export const Sources = () => {
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<SourcesSettingKey, number> = useMemo(() => {
    return {
      wifiType: settings?.wifiType ?? '',
      micType: settings?.micType ?? '',
      // microphone: settings?.microphone ?? 0,
      camera: settings?.camera ?? ''
    }
  }, [settings])

  const { state: settingsState, handleFieldChange, save } = useSmartSettings(initialState, settings)
  const autoSave = () => {}

  const { cameraOptions, cameraIds } = useCameraDetection(initialState, autoSave)

  const cameraValue = coerceSelectValue(initialState.camera ?? '', cameraIds)

  return (
    <SettingsLayout onSave={save}>
      {(Object.keys(sourcesSettingsUIConfig) as SourcesSettingKey[])
        .filter((key) => key !== 'camera')
        .map((key) => {
          const cfg = sourcesSettingsUIConfig[key]

          return (
            <StackItem key={key}>
              <Typography>{sourcesSettingsUIConfig[key].label}</Typography>

              {cfg.customVariantValues && (
                <ToggleButtonGroup
                  value={settingsState[key]}
                  exclusive
                  onChange={(e, value) => handleFieldChange(key, value)}
                  aria-label="text alignment"
                >
                  {cfg.customVariantValues.map((variant, index) => {
                    return (
                      <ToggleButton
                        size="small"
                        value={variant}
                        sx={{
                          minWidth: '100px'
                        }}
                        aria-label={index === 0 ? 'left aligned' : 'justified'}
                      >
                        {variant}
                      </ToggleButton>
                    )
                  })}
                </ToggleButtonGroup>
              )}
            </StackItem>
          )
        })}
      <StackItem>
        <Typography>{sourcesSettingsUIConfig['camera'].label}</Typography>

        <Select
          labelId="select-camera"
          id="select-camera"
          size="small"
          value={cameraValue}
          sx={{
            minWidth: '200px'
          }}
          onChange={(e) => handleFieldChange('camera', e.target.value)}
        >
          {cameraOptions.map((cam) => (
            <MenuItem key={cam.deviceId || 'none'} value={cam.deviceId}>
              {cam.label}
            </MenuItem>
          ))}
        </Select>
      </StackItem>
    </SettingsLayout>
  )
}
