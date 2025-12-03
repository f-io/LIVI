import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { useCarplayStore } from '@store/store'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { WiFiChannelSettingKey } from './types'
import { TextField } from '@mui/material'

export const WifiChannel: React.FC = () => {
  // FIXME types
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<WiFiChannelSettingKey, number | string | boolean> = {
    wifiChannel: settings?.wifiChannel
  }

  const { state: settingsState, handleFieldChange, save } = useSmartSettings(initialState, settings)

  return (
    <SettingsLayout onSave={save}>
      <TextField
        style={{ marginTop: 16 }}
        id="wifiChannel"
        size="small"
        fullWidth
        label="WiFi Channel"
        value={settingsState.wifiChannel ?? ''}
        slotProps={{
          input: { inputProps: { maxLength: 3 } },
          formHelperText: { sx: { textAlign: 'right', m: 0, mt: 0.5 } }
        }}
        helperText={
          "Lorem Ipsum is simply dummy text of the printing and typesetting industry. Lorem Ipsum has been the industry's standard dummy text ever since the 1500s,"
        }
      />
    </SettingsLayout>
  )
}
