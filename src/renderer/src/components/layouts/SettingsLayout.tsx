import Box from '@mui/material/Box'
import { useNavigate } from 'react-router-dom'
import { Button } from '@mui/material'
import Stack from '@mui/material/Stack'
import ArrowBackIosOutlinedIcon from '@mui/icons-material/ArrowBackIosOutlined'
import SaveOutlinedIcon from '@mui/icons-material/SaveOutlined'
import { SettingsLayoutProps } from './types'

export const SettingsLayout = ({ children, onSave }: SettingsLayoutProps) => {
  const navigate = useNavigate()
  const handleNavigate = () => {
    navigate(-1)
  }

  const handleSave = () => {
    const isRequireReset = onSave()

    if (isRequireReset) {
      console.log('need reset app')
    }
  }

  return (
    <Box
      sx={{ width: '100%', px: 3.5, py: 2.25, overflow: 'hidden', height: 'calc(100dvh - 64px)' }}
    >
      <div
        style={{
          overflow: 'auto',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between'
        }}
      >
        <Stack spacing={0}>{children}</Stack>

        <div
          style={{
            display: 'flex',
            flexDirection: 'row',
            justifyContent: 'space-between',
            marginTop: '1rem'
          }}
        >
          <Button
            onClick={handleNavigate}
            sx={{
              width: '100px',
              padding: '0.5rem',
              display: 'flex',
              justifyContent: 'space-between',
              gap: '0.5rem'
            }}
          >
            <ArrowBackIosOutlinedIcon />
            Back
          </Button>

          <Button
            onClick={handleSave}
            sx={{
              width: '100px',
              padding: '0.5rem',
              display: 'flex',
              justifyContent: 'space-between',
              gap: '0.5rem'
            }}
          >
            <SaveOutlinedIcon />
            Save
          </Button>
        </div>
      </div>
    </Box>
  )
}
