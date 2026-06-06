import { Box } from '@mui/material'
import { Clock } from '../../widgets'
import { CENTER_X, NAV_Y } from '../constants'

/** Centred clock — the cluster dash's fallback when no stream is running (no mini-nav, no hole). */
export function ClockCenter() {
  return (
    <Box
      sx={{
        position: 'absolute',
        left: CENTER_X,
        top: NAV_Y,
        transform: 'translate(-50%, -50%)',
        width: 220,
        height: 120,
        display: 'grid',
        placeItems: 'center'
      }}
    >
      <Clock />
    </Box>
  )
}
