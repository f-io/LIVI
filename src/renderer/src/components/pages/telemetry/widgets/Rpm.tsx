import { Box, useTheme } from '@mui/material'
import { SegmentDisplay } from '../components/SegmentDisplay'

export type RpmProps = {
  rpm?: number
  className?: string
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

const SEG_H = 100
const SEG_W = 60
const SEG_S = 9
const RPM_BLOCK_SCALE = 0.45

function segAspect(digits: number) {
  const totalW = digits * SEG_W + (digits - 1) * SEG_S
  return `${totalW} / ${SEG_H}`
}

export function Rpm({ rpm = 0, className }: RpmProps) {
  const theme = useTheme()
  const v = clamp(Math.round(rpm), 0, 9999)

  return (
    <Box
      className={className}
      sx={{
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        display: 'grid',
        alignItems: 'center',
        justifyItems: 'end'
      }}
    >
      {/* ✅ wrapper bestimmt die “echte” Breite von Zahl+Caption */}
      <Box
        sx={{
          width: `calc(100% * ${RPM_BLOCK_SCALE})`,
          minWidth: 0,
          minHeight: 0,
          display: 'grid',
          gridTemplateRows: '1fr auto',
          alignItems: 'center',
          justifyItems: 'end',
          gap: 0.5
        }}
      >
        <Box
          sx={{
            width: '100%',
            minWidth: 0,
            minHeight: 0,
            aspectRatio: segAspect(4),
            display: 'grid',
            alignItems: 'center',
            justifyItems: 'end'
          }}
        >
          <SegmentDisplay
            value={String(v)}
            digits={4}
            onColor={theme.palette.text.primary}
            offColor="transparent"
            offMode="blank"
            style={{ width: '100%', height: '100%', display: 'block' }}
          />
        </Box>

        <Box
          sx={{
            fontSize: 12,
            letterSpacing: 2,
            opacity: 0.7,
            textAlign: 'right',
            color: theme.palette.text.secondary,
            width: '100%'
          }}
        >
          RPM
        </Box>
      </Box>
    </Box>
  )
}
