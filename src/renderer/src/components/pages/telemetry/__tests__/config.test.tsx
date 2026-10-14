import React from 'react'
import { DashboardConfig } from '../config'
import { Dashboard1, Dashboard2, Dashboard3, Dashboard4 } from '../dashboards'
import { TelemetryDashboardIds } from '../types'

vi.mock('../dashboards', () => ({
  Dashboard1: () => React.createElement('div', { 'data-testid': 'dashboard1' }),
  Dashboard2: () => React.createElement('div', { 'data-testid': 'dashboard2' }),
  Dashboard3: () => React.createElement('div', { 'data-testid': 'dashboard3' }),
  Dashboard4: () => React.createElement('div', { 'data-testid': 'dashboard4' })
}))

describe('DashboardConfig', () => {
  test('maps all telemetry dashboard ids to dashboard elements', () => {
    expect(Object.keys(DashboardConfig)).toEqual([
      TelemetryDashboardIds.Dashboard1,
      TelemetryDashboardIds.Dashboard2,
      TelemetryDashboardIds.Dashboard3,
      TelemetryDashboardIds.Dashboard4
    ])

    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dashboard1])).toBe(true)
    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dashboard2])).toBe(true)
    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dashboard3])).toBe(true)
    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dashboard4])).toBe(true)
  })

  test('uses the expected dashboard components for each id', () => {
    expect(DashboardConfig[TelemetryDashboardIds.Dashboard1].type).toBe(Dashboard1)
    expect(DashboardConfig[TelemetryDashboardIds.Dashboard2].type).toBe(Dashboard2)
    expect(DashboardConfig[TelemetryDashboardIds.Dashboard3].type).toBe(Dashboard3)
    expect(DashboardConfig[TelemetryDashboardIds.Dashboard4].type).toBe(Dashboard4)
  })
})
