import { SettingsLayout } from '../../../../layouts/SettingsLayout'
import { StackItem } from '../stackItem'
import { Typography, Switch } from '@mui/material'

export const General = () => {
  return (
    <SettingsLayout>
      <StackItem>
        <Typography>Auto play music</Typography>
        <div>
          <Switch defaultChecked />
        </div>
      </StackItem>
      <StackItem>
        <Typography>Audio</Typography>
        <div>
          <Switch defaultChecked />
        </div>
      </StackItem>
      <StackItem>
        <Typography>Dark mode</Typography>
        <div>
          <Switch defaultChecked />
        </div>
      </StackItem>
      <StackItem>
        <Typography>Fullscreen</Typography>
        <div>
          <Switch defaultChecked />
        </div>
      </StackItem>
    </SettingsLayout>
  )
}
