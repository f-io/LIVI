import type { Dashboards } from '@shared/types'
import { normalizeDashboardComponents } from '../utils'

const slot = (overrides: Partial<Dashboards['dashboard1']> = {}): Dashboards['dashboard1'] => ({
  main: false,
  cluster: false,
  aux: false,
  pos: 1,
  ...overrides
})

describe('normalizeDashboardComponents', () => {
  test('returns empty dashboards when input is null/undefined', () => {
    expect(normalizeDashboardComponents(undefined)).toEqual({ dashboards: [] })
    expect(normalizeDashboardComponents(null)).toEqual({ dashboards: [] })
  })

  test('filters by window role and returns only entries enabled for that role', () => {
    const dashboards: Dashboards = {
      dashboard1: slot({ main: true, pos: 1 }),
      dashboard2: slot({ main: false, cluster: true, pos: 2 }),
      dashboard3: slot({ aux: true, pos: 3 }),
      dashboard4: slot({ pos: 4 })
    }

    expect(normalizeDashboardComponents(dashboards, 'main')).toEqual({
      dashboards: [{ id: 'dashboard1', pos: 1 }]
    })
    expect(normalizeDashboardComponents(dashboards, 'cluster')).toEqual({
      dashboards: [{ id: 'dashboard2', pos: 1 }]
    })
    expect(normalizeDashboardComponents(dashboards, 'aux')).toEqual({
      dashboards: [{ id: 'dashboard3', pos: 1 }]
    })
  })

  test('sorts by position and renumbers to 1..n', () => {
    const dashboards: Dashboards = {
      dashboard1: slot({ main: true, pos: 30 }),
      dashboard2: slot({ main: true, pos: 10 }),
      dashboard3: slot({ main: true, pos: 20 }),
      dashboard4: slot({ pos: 99 })
    }

    expect(normalizeDashboardComponents(dashboards, 'main')).toEqual({
      dashboards: [
        { id: 'dashboard2', pos: 1 },
        { id: 'dashboard3', pos: 2 },
        { id: 'dashboard1', pos: 3 }
      ]
    })
  })

  test('rounds finite positions and pushes invalid positions to the end', () => {
    const dashboards: Dashboards = {
      dashboard1: slot({ main: true, pos: 2.2 }),
      dashboard2: slot({ main: true, pos: Number.NaN }),
      dashboard3: slot({ main: true, pos: 1.6 }),
      dashboard4: slot({ main: true, pos: Infinity })
    }

    expect(normalizeDashboardComponents(dashboards, 'main')).toEqual({
      dashboards: [
        { id: 'dashboard1', pos: 1 },
        { id: 'dashboard3', pos: 2 },
        { id: 'dashboard2', pos: 3 },
        { id: 'dashboard4', pos: 4 }
      ]
    })
  })

  test('defaults to main window role when none is provided', () => {
    const dashboards: Dashboards = {
      dashboard1: slot({ main: true, pos: 1 }),
      dashboard2: slot({ cluster: true, pos: 2 }),
      dashboard3: slot({ pos: 3 }),
      dashboard4: slot({ pos: 4 })
    }

    expect(normalizeDashboardComponents(dashboards)).toEqual({
      dashboards: [{ id: 'dashboard1', pos: 1 }]
    })
  })
})
