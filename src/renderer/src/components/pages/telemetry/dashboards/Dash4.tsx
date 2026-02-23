import { Box } from '@mui/material'
import { NavFull } from '../widgets/NavFull'

export function Dash4() {
  return (
    <Box
      sx={{
        position: 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        display: 'grid',
        placeItems: 'center'
      }}
    >
      <NavFull />
    </Box>
  )
}
