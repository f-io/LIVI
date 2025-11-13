import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import { NavLink } from 'react-router-dom'
import { settingsNestedRoutes } from '../../../routes/settings/nested'
import { dropRight, map } from 'lodash'
import { StackItem } from './componetns/stackItem'

export const NewSettings = () => {
  return (
    <Box
      sx={{
        width: '100%',
        padding: '2rem 1rem 1rem',
        overflow: 'hidden',
        height: 'calc(100dvh - 64px)'
      }}
    >
      <Stack spacing={0} sx={{ overflow: 'auto', height: '100%' }}>
        {map([...dropRight(settingsNestedRoutes, 2)], (item, index) => {
          return (
            <StackItem key={index} withForwardIcon>
              <NavLink to={item.path} key={index}>
                {item.title}
              </NavLink>
            </StackItem>
          )
        })}
      </Stack>
    </Box>
  )
}
