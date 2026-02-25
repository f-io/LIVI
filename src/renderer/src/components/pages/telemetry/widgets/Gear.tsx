import { Box, useTheme } from '@mui/material'

export type GearProps = {
  gear?: string | number
  className?: string
}

function normalizeGear(g: string | number | undefined) {
  const s = String(g ?? '')
    .trim()
    .toUpperCase()
  if (!s || s === 'UNKNOWN') return '—'
  if (s === 'NEUTRAL') return 'N'
  if (s === 'REVERSE') return 'R'
  if (s === 'PARK') return 'P'
  if (s === 'DRIVE') return 'D'
  if (s === 'WINTER') return 'W'
  if (s === 'SPORT') return 'S'
  return s
}

export function Gear({ gear = 'D', className }: GearProps) {
  const theme = useTheme()
  const v = normalizeGear(gear)

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
        justifyItems: 'end',
        gap: 0.5
      }}
    >
      <Box
        sx={{
          width: '100%',
          minWidth: 0,
          minHeight: 0,
          display: 'grid',
          alignItems: 'center',
          justifyItems: 'end'
        }}
      >
        <Box
          sx={{
            fontSize: 88, // ggf. 72..96 je nach gewünschter Optik
            fontWeight: 650,
            lineHeight: 1,
            letterSpacing: '-0.02em',
            color: theme.palette.text.primary
          }}
        >
          {v}
        </Box>
      </Box>

      <Box
        sx={{
          fontSize: 12,
          letterSpacing: 2,
          opacity: 0.7,
          textAlign: 'right',
          color: theme.palette.text.secondary
        }}
      >
        GEAR
      </Box>
    </Box>
  )
}
