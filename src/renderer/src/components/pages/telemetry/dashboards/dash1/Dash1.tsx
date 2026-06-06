import { DashFrame } from './DashFrame'
import { NavMiniCenter } from './NavMiniCenter'

/** Telemetry cluster: gauges + telltales, mini turn-by-turn nav in the centre. */
export function Dash1() {
  return (
    <DashFrame>
      <NavMiniCenter />
    </DashFrame>
  )
}
