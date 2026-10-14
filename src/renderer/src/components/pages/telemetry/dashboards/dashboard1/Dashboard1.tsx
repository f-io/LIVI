import { DashboardFrame } from './DashboardFrame'
import { NavMiniCenter } from './NavMiniCenter'

/** Telemetry cluster: gauges + telltales, mini turn-by-turn nav in the centre. */
export function Dashboard1() {
  return (
    <DashboardFrame>
      <NavMiniCenter />
    </DashboardFrame>
  )
}
