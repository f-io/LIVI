import {
  Dashboard1,
  Dashboard2,
  Dashboard3,
  Dashboard4
} from '@renderer/components/pages/telemetry/dashboards'
import { TelemetryDashboardIds } from '@renderer/components/pages/telemetry/types'

export const DashboardConfig = {
  [TelemetryDashboardIds.Dashboard1]: <Dashboard1 />,
  [TelemetryDashboardIds.Dashboard2]: <Dashboard2 />,
  [TelemetryDashboardIds.Dashboard3]: <Dashboard3 />,
  [TelemetryDashboardIds.Dashboard4]: <Dashboard4 />
}
