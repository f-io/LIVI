import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { useCarplayStore } from '@store/store'
import { useSmartSettings } from '../../hooks/useSmartSettings'
import { iBoxVersionSettingKey } from './types'
import { TextField } from '@mui/material'
import { CAR_NAME_MAX } from '../../../settings/constants'

export const IBoxVersion: React.FC = () => {
  // FIXME types
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
  const settings = useCarplayStore((s) => s.settings)

  const initialState: Record<iBoxVersionSettingKey, number | string | boolean> = {
    iBoxVersion: settings?.iBoxVersion
  }

  const { state: settingsState, handleFieldChange, save } = useSmartSettings(initialState, settings)

  return (
    <SettingsLayout onSave={save}>
      <TextField
        style={{ marginTop: 16 }}
        id="iBoxVersion"
        size="small"
        fullWidth
        label="iBoxVersion"
        value={settingsState.iBoxVersion ?? ''}
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
