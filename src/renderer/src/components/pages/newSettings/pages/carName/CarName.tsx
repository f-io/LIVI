import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { useCarplayStore } from '@store/store'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { ConnectionAndNetworkSettingKey } from './types'
import { TextField } from '@mui/material'
import { CAR_NAME_MAX } from '../../../settings/constants'

export const CarName: React.FC = () => {
  // FIXME types
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<ConnectionAndNetworkSettingKey, number | string | boolean> = {
    carName: settings?.carName
  }

  const { state: settingsState, handleFieldChange, save } = useSmartSettings(initialState, settings)

  return (
    <SettingsLayout onSave={save}>
      <TextField
        style={{ marginTop: 16 }}
        id="carName"
        size="small"
        fullWidth
        label="CAR NAME"
        value={settingsState.carName ?? ''}
        slotProps={{
          input: { inputProps: { maxLength: CAR_NAME_MAX } },
          formHelperText: { sx: { textAlign: 'right', m: 0, mt: 0.5 } }
        }}
        helperText={
          "Lorem Ipsum is simply dummy text of the printing and typesetting industry. Lorem Ipsum has been the industry's standard dummy text ever since the 1500s,"
        }
      />
    </SettingsLayout>
  )
}
