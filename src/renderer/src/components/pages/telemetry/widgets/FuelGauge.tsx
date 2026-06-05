import LocalGasStationIcon from '@mui/icons-material/LocalGasStation'
import { Box, useTheme } from '@mui/material'
import { SegmentBar } from './SegmentBar'

export type FuelGaugeProps = {
  /** Level in percent (0..100). For EVs this is the state-of-charge. */
  level: number
  /** Tank vs battery — drives the icon. Controllable (e.g. from carType). */
  mode?: 'fuel' | 'battery'
  /** Number of bar segments. */
  segments?: number
  /** Icon size in px. */
  size?: number
  /** Value font size in px. */
  valueSize?: number
  /** Warn (red) at/below this percent. */
  warnBelow?: number
  className?: string
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v))

/** Small horizontal battery outline (MUI battery glyphs are vertical). */
function BatteryGlyph({ size, color }: { size: number; color: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <rect x="2.5" y="8" width="17" height="8" rx="1.8" stroke={color} strokeWidth="1.6" />
      <rect x="20" y="10.5" width="2.2" height="3" rx="0.6" fill={color} />
    </svg>
  )
}

/**
 * Compact fuel / charge readout: a type icon (fuel pump or battery), a segmented
 * level bar, and the percentage. Turns red below `warnBelow`. The `mode` is controllable so the
 * same widget serves combustion and EV.
 */
export function FuelGauge({
  level,
  mode = 'fuel',
  segments = 8,
  size = 22,
  valueSize = 22,
  warnBelow = 12,
  className
}: FuelGaugeProps) {
  const theme = useTheme()
  const v = Number.isFinite(level) ? clamp(Math.round(level), 0, 100) : 0
  const low = v <= warnBelow
  const on = low ? theme.palette.error.main : theme.palette.text.primary
  const iconColor = low ? theme.palette.error.main : theme.palette.text.secondary

  return (
    <Box className={className} sx={{ display: 'flex', alignItems: 'center', gap: 1.1 }}>
      {mode === 'battery' ? (
        <BatteryGlyph size={size} color={iconColor} />
      ) : (
        <LocalGasStationIcon sx={{ fontSize: size, color: iconColor }} />
      )}

      <SegmentBar
        ratio={v / 100}
        segments={segments}
        onColor={on}
        offColor={theme.palette.text.disabled}
      />

      <Box
        sx={{
          fontSize: valueSize,
          fontWeight: 400,
          letterSpacing: '-0.01em',
          color: on,
          fontVariantNumeric: 'tabular-nums',
          minWidth: '3ch',
          textAlign: 'right'
        }}
      >
        {v}%
      </Box>
    </Box>
  )
}
