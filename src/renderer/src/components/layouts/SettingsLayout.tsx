import Box from '@mui/material/Box'
import { useNavigate } from 'react-router-dom'
import { Button } from '@mui/material'
import Stack from '@mui/material/Stack'
import ArrowBackIosOutlinedIcon from '@mui/icons-material/ArrowBackIosOutlined'

export const SettingsLayout = ({ children }) => {
  const navigate = useNavigate()
  const handleNavigate = () => {
    navigate(-1)
  }

  return (
    <Box sx={{ width: '100%', p: 3, overflow: 'hidden', height: 'calc(100dvh - 64px)' }}>
      <div style={{ overflow: 'auto', height: '100%' }}>
        <Button
          onClick={handleNavigate}
          sx={{
            width: '100px',
            padding: '0.5rem',
            display: 'flex',
            justifyContent: 'space-between',
            gap: '0.5rem',
            marginBottom: '0.5rem'
          }}
        >
          <ArrowBackIosOutlinedIcon />
          Back
        </Button>

        <Stack spacing={0}>{children}</Stack>
      </div>
    </Box>
  )
}
