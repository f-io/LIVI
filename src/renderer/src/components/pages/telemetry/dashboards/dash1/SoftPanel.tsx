import { Box, type SxProps, type Theme } from '@mui/material'

export type SoftPanelProps = {
  /** Peak opacity at the centre (0..1). */
  alpha?: number
  /** % radius that stays fully opaque before the fade starts. */
  soft?: number
  /** % radius at which it is fully transparent. */
  end?: number
  /** radial-gradient ending shape, e.g. 'ellipse 58% 56%' (radii relative to the box). */
  shape?: string
  /** Centre of the glow in % of the box. Sit it behind the 0/P readout so it stays legible. */
  originX?: number
  originY?: number
  sx?: SxProps<Theme>
}

/**
 * A soft backdrop that fades to transparent towards its edges (radial gradient, no hard box).
 * Sits behind a dash widget on the full-map cluster dash so the instrument stays legible while the
 * cluster stream shows through everywhere else. Theme-aware: dark in dark mode, white in light mode.
 */
export function SoftPanel({
  alpha = 0.8,
  soft = 50,
  end = 90,
  shape = 'ellipse 60% 60%',
  originX = 50,
  originY = 50,
  sx
}: SoftPanelProps) {
  const at = `${originX}% ${originY}%`
  return (
    <Box
      sx={[
        (theme) => {
          const rgb = theme.palette.mode === 'light' ? '255, 255, 255' : '0, 0, 0'
          return {
            position: 'absolute',
            inset: 0,
            pointerEvents: 'none',
            background: `radial-gradient(${shape} at ${at}, rgba(${rgb}, ${alpha}) 0%, rgba(${rgb}, ${alpha}) ${soft}%, rgba(${rgb}, 0) ${end}%)`
          }
        },
        ...(Array.isArray(sx) ? sx : [sx])
      ]}
    />
  )
}
