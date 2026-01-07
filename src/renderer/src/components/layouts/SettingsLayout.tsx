import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import IconButton from '@mui/material/IconButton'
import Typography from '@mui/material/Typography'
import ArrowBackIosOutlinedIcon from '@mui/icons-material/ArrowBackIosOutlined'
import RestartAltOutlinedIcon from '@mui/icons-material/RestartAltOutlined'
import { useLocation, useNavigate } from 'react-router'
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
  const location = useLocation()

  const handleNavigate = () => navigate(-1)

  const isShouldShowBackButton = location.pathname !== '/settings'

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
        pl: 'clamp(12px, 1.5dvw, 28px)',
        pr: 'clamp(12px, 3.5dvw, 28px)',
        pt: 'clamp(8px, 2.2dvh, 18px)',
        pb: 'clamp(10px, 2.2dvh, 18px)',
        gap: '0.75rem'
      }}
    >
      {/* HEADER */}
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns:
            !isShouldShowBackButton && !showRestart
              ? '1fr'
              : 'clamp(36px, 6dvw, 56px) 1fr clamp(36px, 8dvw, 100px)',
          alignItems: 'center',
          flex: '0 0 auto',
          padding: '0, 0.5rem',
          height: 'clamp(32px, 5.5dvw, 44px)'
        }}
      >
        {isShouldShowBackButton && (
          <Box sx={{ display: 'flex', alignItems: 'center' }}>
            <IconButton
              onClick={handleNavigate}
              aria-label="Back"
              sx={{
                width: 'clamp(32px, 5.5dvw, 44px)'
              }}
            >
              <ArrowBackIosOutlinedIcon />
            </IconButton>
          </Box>
        )}

        <Typography
          sx={{
            textAlign: 'center',
            fontWeight: 800,
            lineHeight: 1.05,
            fontSize: 'clamp(16px, 3.6dvh, 34px)'
          }}
        >
          {title}
        </Typography>

        {showRestart && (
          <Box sx={{ display: 'flex', justifyContent: 'flex-end', alignItems: 'center' }}>
            <IconButton
              onClick={onRestart}
              aria-label="Restart dongle"
              sx={{
                width: '100%',
                color: theme.palette.primary.main
              }}
            >
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  whiteSpace: 'nowrap',
                  fontSize: '1rem',
                  gap: '0.5rem'
                }}
              >
                <span>Apply</span>
                <RestartAltOutlinedIcon />
              </div>
            </IconButton>
          </Box>
        )}
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
        <Stack spacing={0} sx={{ minHeight: 0, padding: '0 0 0 0.5rem' }}>
          {children}
        </Stack>
      </Box>
    </Box>
  )
}
