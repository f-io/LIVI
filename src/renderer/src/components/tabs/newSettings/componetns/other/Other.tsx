import { settingsSubNestedPath } from '@renderer/routes/settings/nested'
import { SettingsLayout } from '@renderer/components/layouts/SettingsLayout'
import { StackItem } from '../StackItem'
import { Typography } from '@mui/material'
import { NavLink } from 'react-router-dom'

export const Other = () => {
  return (
    <SettingsLayout>
      <StackItem>
        <NavLink to={settingsSubNestedPath.viewMode}>
          <Typography>ViewMode</Typography>
        </NavLink>
      </StackItem>

      <StackItem>
        <NavLink to={settingsSubNestedPath.advanced}>
          <Typography>Advanced</Typography>
        </NavLink>
      </StackItem>
    </SettingsLayout>
  )
}
