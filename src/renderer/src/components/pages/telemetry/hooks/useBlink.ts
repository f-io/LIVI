import * as React from 'react'

/**
 * Boolean that flips on/off every `periodMs` (default 500 ms), phase-aligned to the wall
 * clock so every consumer blinks in sync
 */
export function useBlink(periodMs = 500): boolean {
  const [on, setOn] = React.useState(true)

  React.useEffect(() => {
    const tick = () => setOn(Math.floor(Date.now() / periodMs) % 2 === 0)
    tick()
    const sample = Math.max(20, Math.floor(periodMs / 10))
    const id = setInterval(tick, sample)
    return () => clearInterval(id)
  }, [periodMs])

  return on
}
