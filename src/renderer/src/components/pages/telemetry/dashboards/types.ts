import type { TelemetryDashboardId } from '@main/Globals'

export type DashboardDef = {
  id: TelemetryDashboardId
  Component: React.ComponentType
}
