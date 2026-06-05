import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import { useBlink } from '../hooks/useBlink'

// ECE R121 turn-signal green, bright for low-contrast clusters.
const SIGNAL_GREEN = '#19e04b'

export type TurnArrowProps = {
  side: 'left' | 'right'
  /** Flash this arrow (turn on that side, or hazards). */
  active: boolean
  /** Arrow height in px. */
  size?: number
}

/**
 * A single turn-signal arrow for the outer edge of the cluster. Flashes at 500 ms via the
 * shared useBlink clock, so both arrows and the hazard triangle stay in sync.
 */
export function TurnArrow({ side, active, size = 56 }: TurnArrowProps) {
  const on = useBlink(500)
  const lit = active && on
  return (
    <PlayArrowIcon
      data-testid={`turn-${side}`}
      style={{ opacity: lit ? 1 : 0 }}
      sx={{
        fontSize: size,
        color: SIGNAL_GREEN,
        transform: side === 'left' ? 'rotate(180deg)' : 'none'
      }}
    />
  )
}
