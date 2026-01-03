import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import IconButton from '@mui/material/IconButton'
import Typography from '@mui/material/Typography'
import ArrowBackIosOutlinedIcon from '@mui/icons-material/ArrowBackIosOutlined'
import RestartAltOutlinedIcon from '@mui/icons-material/RestartAltOutlined'
import { useNavigate } from 'react-router'
import { SettingsLayoutProps } from './types'
import { useTheme } from '@mui/material/styles'

export const SettingsLayout = ({
  children,
  title,
  showRestart,
  onRestart
}: SettingsLayoutProps) => {
  const navigate = useNavigate()
  const theme = useTheme()

  const handleNavigate = () => navigate(-1)

  return (
    <Box
      sx={{
        position: 'absolute',
        inset: 0,
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        overflow: 'hidden',
        boxSizing: 'border-box',
        px: 'clamp(12px, 3.5vw, 28px)',
        pt: 'clamp(8px, 2.2vh, 18px)',
        pb: 'clamp(10px, 2.2vh, 18px)'
      }}
    >
      {/* HEADER */}
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'clamp(36px, 6vw, 56px) 1fr clamp(36px, 6vw, 56px)',
          alignItems: 'center',
          flex: '0 0 auto',
          mb: 'clamp(8px, 1.5vh, 16px)'
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center' }}>
          <IconButton
            onClick={handleNavigate}
            aria-label="Back"
            sx={{
              width: 'clamp(32px, 5.5vw, 44px)',
              height: 'clamp(32px, 5.5vw, 44px)'
            }}
          >
            <ArrowBackIosOutlinedIcon />
          </IconButton>
        </Box>

        <Typography
          sx={{
            textAlign: 'center',
            fontWeight: 800,
            lineHeight: 1.05,
            fontSize: 'clamp(16px, 3.6vh, 34px)'
          }}
        >
          {title}
        </Typography>

        <Box sx={{ display: 'flex', justifyContent: 'flex-end', alignItems: 'center' }}>
          <IconButton
            onClick={onRestart}
            aria-label="Restart dongle"
            sx={{
              width: 'clamp(32px, 5.5vw, 44px)',
              height: 'clamp(32px, 5.5vw, 44px)',
              opacity: showRestart ? 1 : 0,
              pointerEvents: showRestart ? 'auto' : 'none',
              color: showRestart ? theme.palette.primary.main : 'inherit'
            }}
          >
            <RestartAltOutlinedIcon />
          </IconButton>
        </Box>
      </Box>

      <Box
        sx={{
          flex: '1 1 auto',
          minHeight: 0,
          overflowY: 'auto',
          overflowX: 'hidden',
          WebkitOverflowScrolling: 'touch',
          touchAction: 'pan-y'
        }}
      >
        <Stack spacing={0} sx={{ minHeight: 0 }}>
          {children}
        </Stack>
      </Box>
    </Box>
  )
}
