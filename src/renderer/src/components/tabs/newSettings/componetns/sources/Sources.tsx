import { SettingsLayout } from '../../../../layouts/SettingsLayout'
import { StackItem } from '../stackItem'
import { Typography, TextField, MenuItem } from '@mui/material'
import { WiFiValues } from '../../../settings/constants'

export const Sources = () => {
  return (
    <SettingsLayout>
      <StackItem>
        <Typography>WiFi</Typography>
        <div>
          <TextField
            id="wifi"
            size="small"
            select
            fullWidth
            label="WIFI"
            value={WiFiValues['2.4ghz']}
          >
            <MenuItem value={WiFiValues['2.4ghz']}>2.4 GHz</MenuItem>
            <MenuItem value={WiFiValues['5ghz']}>5 GHz</MenuItem>
          </TextField>
        </div>
      </StackItem>
    </SettingsLayout>
  )
}
