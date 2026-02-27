import { TelemetryDashboardIds } from '@renderer/components/pages/telemetry/types'
import { Dash1, Dash2, Dash3, Dash4 } from '@renderer/components/pages/telemetry/dashboards'

export const DashboardConfig = {
  [TelemetryDashboardIds.Dash1]: <Dash1 />,
  [TelemetryDashboardIds.Dash2]: <Dash2 />,
  [TelemetryDashboardIds.Dash3]: <Dash3 />,
  [TelemetryDashboardIds.Dash4]: <Dash4 />
}
