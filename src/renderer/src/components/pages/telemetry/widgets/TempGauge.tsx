import DeviceThermostatIcon from '@mui/icons-material/DeviceThermostat'
import { Box, useTheme } from '@mui/material'
import { SegmentBar } from './SegmentBar'

export type TempGaugeProps = {
  /** Temperature in °C. */
  value: number
  /** Bar start / end (°C). */
  min?: number
  max?: number
  /** Warn (red) at/above this temperature. */
  warnAbove?: number
  segments?: number
  size?: number
  valueSize?: number
  className?: string
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v))

/**
 * Compact temperature readout in the same language as FuelGauge: a thermometer icon, a segmented
 * bar from `min`…`max`, and the value in °C. Turns red at/above `warnAbove`. Used for engine oil
 * temperature.
 */
export function TempGauge({
  value,
  min = 40,
  max = 150,
  warnAbove = 125,
  segments = 8,
  size = 22,
  valueSize = 22,
  className
}: TempGaugeProps) {
  const theme = useTheme()
  const v = Number.isFinite(value) ? Math.round(value) : 0
  const ratio = (clamp(v, min, max) - min) / Math.max(1, max - min)
  const hot = v >= warnAbove
  const on = hot ? theme.palette.error.main : theme.palette.text.primary
  const iconColor = hot ? theme.palette.error.main : theme.palette.text.secondary

  return (
    <Box className={className} sx={{ display: 'flex', alignItems: 'center', gap: 1.1 }}>
      <DeviceThermostatIcon sx={{ fontSize: size, color: iconColor }} />

      <SegmentBar
        ratio={ratio}
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
        {v}°
      </Box>
    </Box>
  )
}
