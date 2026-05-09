import type { DashboardsConfig } from '@shared/types'
import { normalizeDashComponents } from '../utils'

const slot = (overrides: Partial<DashboardsConfig['dash1']> = {}): DashboardsConfig['dash1'] => ({
  main: false,
  dash: false,
  aux: false,
  pos: 1,
  ...overrides
})

describe('normalizeDashComponents', () => {
  test('returns empty dashboards when input is null/undefined', () => {
    expect(normalizeDashComponents(undefined)).toEqual({ dashboards: [] })
    expect(normalizeDashComponents(null)).toEqual({ dashboards: [] })
  })

  test('filters by window role and returns only entries enabled for that role', () => {
    const dashboards: DashboardsConfig = {
      dash1: slot({ main: true, pos: 1 }),
      dash2: slot({ main: false, dash: true, pos: 2 }),
      dash3: slot({ aux: true, pos: 3 }),
      dash4: slot({ pos: 4 })
    }

    expect(normalizeDashComponents(dashboards, 'main')).toEqual({
      dashboards: [{ id: 'dash1', pos: 1 }]
    })
    expect(normalizeDashComponents(dashboards, 'dash')).toEqual({
      dashboards: [{ id: 'dash2', pos: 1 }]
    })
    expect(normalizeDashComponents(dashboards, 'aux')).toEqual({
      dashboards: [{ id: 'dash3', pos: 1 }]
    })
  })

  test('sorts by position and renumbers to 1..n', () => {
    const dashboards: DashboardsConfig = {
      dash1: slot({ main: true, pos: 30 }),
      dash2: slot({ main: true, pos: 10 }),
      dash3: slot({ main: true, pos: 20 }),
      dash4: slot({ pos: 99 })
    }

    expect(normalizeDashComponents(dashboards, 'main')).toEqual({
      dashboards: [
        { id: 'dash2', pos: 1 },
        { id: 'dash3', pos: 2 },
        { id: 'dash1', pos: 3 }
      ]
    })
  })

  test('rounds finite positions and pushes invalid positions to the end', () => {
    const dashboards: DashboardsConfig = {
      dash1: slot({ main: true, pos: 2.2 }),
      dash2: slot({ main: true, pos: Number.NaN }),
      dash3: slot({ main: true, pos: 1.6 }),
      dash4: slot({ main: true, pos: Infinity })
    }

    expect(normalizeDashComponents(dashboards, 'main')).toEqual({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash3', pos: 2 },
        { id: 'dash2', pos: 3 },
        { id: 'dash4', pos: 4 }
      ]
    })
  })

  test('defaults to main window role when none is provided', () => {
    const dashboards: DashboardsConfig = {
      dash1: slot({ main: true, pos: 1 }),
      dash2: slot({ dash: true, pos: 2 }),
      dash3: slot({ pos: 3 }),
      dash4: slot({ pos: 4 })
    }

    expect(normalizeDashComponents(dashboards)).toEqual({
      dashboards: [{ id: 'dash1', pos: 1 }]
    })
  })
})
