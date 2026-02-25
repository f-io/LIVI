import { Box, Typography, useTheme } from '@mui/material'

type Align = 'left' | 'center' | 'right'

export type MetricRowProps = {
  className?: string

  label: string

  value: number | string
  unit?: string

  // bar / range
  min: number
  max: number
  barValue: number

  warnFrom?: number
  warnBelow?: number

  // layout
  valueWidthCh?: number
  labelAlign?: Align
}

const clamp = (v: number, min: number, max: number) => Math.max(min, Math.min(max, v))

const LABEL_FS = 12
const VALUE_FS = 32
const UNIT_FS = 18

const BAR_W = 80
const BAR_H = 5

const LABEL_W = 68
const VALUE_W_CH = 6
const UNIT_W_CH = 2 // "°C" / "%" fits

export function MetricRow({
  className,
  label,
  value,
  unit,
  min,
  max,
  barValue,
  warnFrom,
  warnBelow,
  labelAlign = 'right'
}: MetricRowProps) {
  const theme = useTheme()

  const safeMin = Number.isFinite(min) ? min : 0
  const safeMax = Number.isFinite(max) && max > safeMin ? max : safeMin + 1
  const v = Number.isFinite(barValue) ? barValue : safeMin

  const ratio = clamp((v - safeMin) / (safeMax - safeMin), 0, 1)
  const isWarn =
    (typeof warnFrom === 'number' && v >= warnFrom) ||
    (typeof warnBelow === 'number' && v <= warnBelow)

  const labelTextAlign: 'left' | 'center' | 'right' =
    labelAlign === 'left' ? 'left' : labelAlign === 'center' ? 'center' : 'right'

  return (
    <Box
      className={className}
      sx={{
        display: 'grid',
        alignItems: 'center',
        columnGap: 1.25,
        // label | value | unit | bar
        gridTemplateColumns: `${LABEL_W}px ${VALUE_W_CH}ch ${UNIT_W_CH}ch ${BAR_W}px`
      }}
    >
      {/* label */}
      <Typography
        sx={{
          fontSize: LABEL_FS,
          letterSpacing: 3,
          opacity: 0.7,
          color: theme.palette.text.secondary,
          lineHeight: 1,
          whiteSpace: 'nowrap',
          textAlign: labelTextAlign
        }}
      >
        {label}
      </Typography>

      {/* value */}
      <Typography
        sx={{
          fontSize: VALUE_FS,
          lineHeight: 1,
          color: theme.palette.text.primary,
          fontVariantNumeric: 'tabular-nums',
          width: `${VALUE_W_CH}ch`,
          textAlign: 'right',
          justifySelf: 'end'
        }}
      >
        {value}
      </Typography>

      {/* unit */}
      <Typography
        sx={{
          fontSize: UNIT_FS,
          lineHeight: 1,
          opacity: 0.7,
          color: theme.palette.text.secondary,
          whiteSpace: 'nowrap',
          width: `${UNIT_W_CH}ch`,
          textAlign: 'right',
          justifySelf: 'end',
          alignSelf: 'center',
          transform: 'translateY(2px)'
        }}
      >
        {unit ?? ''}
      </Typography>

      {/* micro bar */}
      <Box
        sx={{
          width: BAR_W,
          height: BAR_H,
          borderRadius: 999,
          position: 'relative',
          bgcolor: 'rgba(255,255,255,0.14)',
          boxShadow: '0 0 0 1px rgba(255,255,255,0.06) inset'
        }}
      >
        <Box
          sx={{
            position: 'absolute',
            left: 0,
            top: 0,
            height: '100%',
            width: `${Math.round(ratio * 100)}%`,
            borderRadius: 999,
            bgcolor: isWarn ? theme.palette.error.main : theme.palette.text.primary,
            opacity: isWarn ? 0.9 : 0.55
          }}
        />
        <Box
          sx={{
            position: 'absolute',
            left: `${ratio * 100}%`,
            top: -2,
            height: BAR_H + 4,
            width: 2,
            transform: 'translateX(-50%)',
            borderRadius: 999,
            bgcolor: isWarn ? theme.palette.error.main : theme.palette.text.primary,
            opacity: isWarn ? 0.95 : 0.75,
            boxShadow: isWarn ? '0 0 8px rgba(255,0,0,0.28)' : '0 0 8px rgba(255,255,255,0.18)'
          }}
        />
      </Box>
    </Box>
  )
}
