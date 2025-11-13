import {
  Box,
  Dialog,
  DialogTitle,
  DialogContent,
  DialogActions,
  TextField,
  Button
} from '@mui/material'

export const ScreenResolution = ({ isOpen, onClose }) => {
  return (
    <Dialog
      open={isOpen}
      // keepMounted
      onClose={() => onClose(false)}
    >
      <DialogTitle>Set screen resolution</DialogTitle>
      <DialogContent>
        <Box
          sx={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 1, marginTop: 1 }}
        >
          <TextField fullWidth id="outlined-basic" label="Width" variant="outlined" />
          <span>X</span>
          <TextField fullWidth id="outlined-basic" label="Height" variant="outlined" />
        </Box>
      </DialogContent>
      <DialogActions>
        <Button onClick={() => onClose(false)}>Close</Button>
      </DialogActions>
    </Dialog>
  )
}
