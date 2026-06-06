import { NavFull } from '../../widgets'
import { DashFrame } from '../dash1/DashFrame'

/** Gauges + telltales with the full turn-by-turn navigation in the centre. */
export function Dash2() {
  return (
    <DashFrame>
      <NavFull />
    </DashFrame>
  )
}
