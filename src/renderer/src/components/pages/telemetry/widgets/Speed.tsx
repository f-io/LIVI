import { Box, useTheme } from '@mui/material'
import { SegmentDisplay } from '../components/SegmentDisplay'

export type SpeedProps = {
  speedKph?: number
  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

const SEG_H = 100
const SEG_W = 60
const SEG_S = 9

function segAspect(digits: number) {
  const totalW = digits * SEG_W + (digits - 1) * SEG_S
  return `${totalW} / ${SEG_H}`
}

export function Speed({ speedKph = 0, className }: SpeedProps) {
  const theme = useTheme()
  const v = clamp(Math.round(speedKph), 0, 999)

  return (
    <Box
      className={className}
      sx={{
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        display: 'grid',
        gridTemplateRows: '1fr auto',
        alignItems: 'center',
        justifyItems: 'center',
        gap: 1
      }}
    >
      <Box
        sx={{
          width: '100%',
          minWidth: 0,
          minHeight: 0,
          aspectRatio: segAspect(3),
          display: 'grid',
          alignItems: 'center',
          justifyItems: 'center'
        }}
      >
        <SegmentDisplay
          value={String(v)}
          digits={3}
          onColor={theme.palette.text.primary}
          offColor="transparent"
          offMode="blank"
          style={{ width: '100%', height: '100%', display: 'block' }}
        />
      </Box>

      <Box
        sx={{
          justifySelf: 'end',
          width: '100%',
          fontSize: 12,
          letterSpacing: 2,
          opacity: 0.7,
          textAlign: 'right',
          color: theme.palette.text.secondary
        }}
      >
        KPH
      </Box>
    </Box>
  )
}
