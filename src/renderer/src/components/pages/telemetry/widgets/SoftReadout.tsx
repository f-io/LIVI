import { Box, useTheme } from '@mui/material'

export type SoftReadoutProps = {
  /** Big value (speed number, gear letter, …). */
  value: string | number
  /** Small caption below (e.g. "KPH", "GEAR"). */
  label: string
  /** Horizontal alignment of the stack. */
  align?: 'center' | 'start' | 'end'
  /** Reserve this many character-widths and align the value to the fixed edge */
  maxChars?: number
  /** Big-value font size in px. */
  size?: number
  className?: string
}

/**
 * Soft cluster readout: a large value over a small caption
 */
export function SoftReadout({
  value,
  label,
  align = 'center',
  maxChars,
  size = 96,
  className
}: SoftReadoutProps) {
  const theme = useTheme()
  const flexAlign = align === 'end' ? 'flex-end' : align === 'start' ? 'flex-start' : 'center'
  const textAlign = align === 'end' ? 'right' : align === 'start' ? 'left' : 'center'
  return (
    <Box
      className={className}
      sx={{
        width: '100%',
        height: '100%',
        minWidth: 0,
        minHeight: 0,
        display: 'grid',
        placeItems: 'center'
      }}
    >
      {/* Shrink-wrapped to the value width so the caption can hug the value's anchored edge. */}
      <Box
        sx={{
          display: 'inline-flex',
          flexDirection: 'column',
          alignItems: flexAlign
        }}
      >
        <Box
          sx={{
            fontSize: size,
            fontWeight: 400,
            lineHeight: 1,
            letterSpacing: '-0.01em',
            color: theme.palette.text.primary,
            fontVariantNumeric: 'tabular-nums',
            ...(maxChars ? { minWidth: `${maxChars}ch`, textAlign } : {})
          }}
        >
          {value}
        </Box>
        <Box
          sx={{
            mt: '2px',
            fontSize: 10,
            fontWeight: 500,
            letterSpacing: 1.5,
            opacity: 0.5,
            color: theme.palette.text.secondary
          }}
        >
          {label}
        </Box>
      </Box>
    </Box>
  )
}
