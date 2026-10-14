import { NavFull } from '../../widgets'
import { DashboardFrame } from '../dashboard1/DashboardFrame'

/** Gauges + telltales with the full turn-by-turn navigation in the centre. */
export function Dashboard2() {
  return (
    <DashboardFrame>
      <NavFull />
    </DashboardFrame>
  )
}
