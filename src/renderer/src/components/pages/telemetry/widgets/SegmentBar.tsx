import { Box } from '@mui/material'

export type SegmentBarProps = {
  /** Fill fraction 0..1. */
  ratio: number
  segments?: number
  onColor: string
  offColor: string
  className?: string
}

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v))

/** Shared segmented level bar (fuel, charge, temperature, …). */
export function SegmentBar({ ratio, segments = 8, onColor, offColor, className }: SegmentBarProps) {
  const filled = Math.round(clamp(ratio, 0, 1) * segments)
  return (
    <Box className={className} sx={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
      {Array.from({ length: segments }, (_, i) => (
        <Box
          key={i}
          sx={{
            width: 12,
            height: 8,
            borderRadius: '2px',
            backgroundColor: i < filled ? onColor : offColor,
            opacity: i < filled ? 1 : 0.5
          }}
        />
      ))}
    </Box>
  )
}
