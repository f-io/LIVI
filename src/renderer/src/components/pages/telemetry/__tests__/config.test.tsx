import React from 'react'
import { DashboardConfig } from '../config'
import { TelemetryDashboardIds } from '../types'
import { Dash1, Dash2, Dash3, Dash4 } from '../dashboards'

jest.mock('../dashboards', () => ({
  Dash1: () => React.createElement('div', { 'data-testid': 'dash1' }),
  Dash2: () => React.createElement('div', { 'data-testid': 'dash2' }),
  Dash3: () => React.createElement('div', { 'data-testid': 'dash3' }),
  Dash4: () => React.createElement('div', { 'data-testid': 'dash4' })
}))

describe('DashboardConfig', () => {
  test('maps all telemetry dashboard ids to dashboard elements', () => {
    expect(Object.keys(DashboardConfig)).toEqual([
      TelemetryDashboardIds.Dash1,
      TelemetryDashboardIds.Dash2,
      TelemetryDashboardIds.Dash3,
      TelemetryDashboardIds.Dash4
    ])

    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dash1])).toBe(true)
    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dash2])).toBe(true)
    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dash3])).toBe(true)
    expect(React.isValidElement(DashboardConfig[TelemetryDashboardIds.Dash4])).toBe(true)
  })

  test('uses the expected dashboard components for each id', () => {
    expect(DashboardConfig[TelemetryDashboardIds.Dash1].type).toBe(Dash1)
    expect(DashboardConfig[TelemetryDashboardIds.Dash2].type).toBe(Dash2)
    expect(DashboardConfig[TelemetryDashboardIds.Dash3].type).toBe(Dash3)
    expect(DashboardConfig[TelemetryDashboardIds.Dash4].type).toBe(Dash4)
  })
})
