import { Box, Typography, useTheme } from '@mui/material'
import { UI } from '../../../../constants'
import { useBlinkingTime } from '../../../../hooks/useBlinkingTime'

export type ClockProps = {
  className?: string
  /** Font size in px. */
  size?: number
}

/**
 * Standalone HH:MM clock with a blinking colon. Used as the nav centre's no-route fallback and as
 * the cluster dashboard's centre when no stream is running.
 */
export function Clock({ className, size }: ClockProps) {
  const theme = useTheme()
  const clockText = useBlinkingTime()
  const showColon = clockText.includes(':')
  const [hh, mm] = clockText.replace(' ', ':').split(':')

  const isXSIcons = typeof window !== 'undefined' && window.innerHeight <= UI.XS_ICON_MAX_HEIGHT
  const resolvedSize = size ?? (isXSIcons ? 60 : 44)

  return (
    <Box
      className={className}
      sx={{ width: '100%', height: '100%', display: 'grid', placeItems: 'center' }}
    >
      <Typography
        sx={{
          fontSize: resolvedSize,
          fontWeight: 400,
          lineHeight: 1,
          color: theme.palette.text.primary,
          fontVariantNumeric: 'tabular-nums',
          letterSpacing: 1.9,
          opacity: 0.55
        }}
      >
        {hh}
        <Box
          component="span"
          sx={{ opacity: showColon ? 1 : 0, transition: 'opacity 120ms linear' }}
        >
          :
        </Box>
        {mm}
      </Typography>
    </Box>
  )
}
