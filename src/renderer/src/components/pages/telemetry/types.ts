export enum TelemetryDashboardIds {
  Dash1 = 'dash1',
  Dash2 = 'dash2',
  Dash3 = 'dash3',
  Dash4 = 'dash4'
}

export type TelemetryDashboardItemProp = {
  id: TelemetryDashboardIds
  enabled: boolean
  pos: number
}
