import { TelemetryDashboardIds } from '@renderer/components/pages/telemetry/types'
import { Dash1 } from '@renderer/components/pages/telemetry/dashboards/Dash1'
import { Dash2 } from '@renderer/components/pages/telemetry/dashboards/Dash2'
import { Dash3 } from '@renderer/components/pages/telemetry/dashboards/Dash3'
import { Dash4 } from '@renderer/components/pages/telemetry/dashboards/Dash4'

export const DashboardConfig = {
  [TelemetryDashboardIds.Dash1]: <Dash1 />,
  [TelemetryDashboardIds.Dash2]: <Dash2 />,
  [TelemetryDashboardIds.Dash3]: <Dash3 />,
  [TelemetryDashboardIds.Dash4]: <Dash4 />
}
