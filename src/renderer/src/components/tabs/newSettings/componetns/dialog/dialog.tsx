import { Box, Button, Dialog, DialogTitle, DialogContent, DialogActions } from '@mui/material'
import { map } from 'lodash'
import { DialogProps } from './types'
import { SETTINGS_DIALOGS } from '../types'

export const SettingsDialog = ({
  children,
  type,
  title,
  actions,
  isOpen,
  onClose
}: DialogProps) => {
  return (
    <Dialog open={isOpen} onClose={() => onClose(type as SETTINGS_DIALOGS, false)}>
      <DialogTitle>{title}</DialogTitle>

      <DialogContent>
        <Box
          sx={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 1, marginTop: 1 }}
        >
          {children}
        </Box>
      </DialogContent>

      {!!actions && (
        <DialogActions>
          <Button onClick={() => onClose(type as SETTINGS_DIALOGS, false)}>Close</Button>
          {map(actions, (action, index) => (
            <Button onClick={() => action.handler()} key={index}>
              {action.title}
            </Button>
          ))}
        </DialogActions>
      )}
    </Dialog>
  )
}
