export enum TelemetryDashboardIds {
  Dashboard1 = 'dashboard1',
  Dashboard2 = 'dashboard2',
  Dashboard3 = 'dashboard3',
  Dashboard4 = 'dashboard4'
}

export type TelemetryDashboardItemProp = {
  id: TelemetryDashboardIds
  enabled: boolean
  pos: number
}
