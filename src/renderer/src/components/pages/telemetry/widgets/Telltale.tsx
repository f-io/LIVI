import { Box } from '@mui/material'
import type * as React from 'react'
import { useBlink } from '../hooks/useBlink'

export type TelltaleProps = {
  /** Lamp is on (function active). */
  active: boolean
  /** Standardised signal colour (ECE R121): red / amber / green / blue. */
  color: string
  /** Flash at 500 ms (turn signals); static lamps stay solid while active. */
  blink?: boolean
  /** Box edge length in px. */
  size?: number
  /** Optional test hook. */
  testId?: string
  /** The glyph (an svg using currentColor). */
  children: React.ReactNode
}

/**
 * One cluster telltale lamp. Solid in `color` while active, invisible while off
 */
export function Telltale({
  active,
  color,
  blink = false,
  size = 30,
  testId,
  children
}: TelltaleProps) {
  const on = useBlink(500)
  const lit = active && (blink ? on : true)
  return (
    <Box
      data-testid={testId}
      style={{ opacity: lit ? 1 : 0 }}
      sx={{ width: size, height: size, color, display: 'grid', placeItems: 'center' }}
    >
      {children}
    </Box>
  )
}
