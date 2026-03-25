import { normalizeDashComponents } from '../utils'

describe('normalizeDashComponents', () => {
  test('returns empty dashboards when input is not an array', () => {
    expect(normalizeDashComponents(undefined)).toEqual({ dashboards: [] })
    expect(normalizeDashComponents(null)).toEqual({ dashboards: [] })
    expect(normalizeDashComponents({})).toEqual({ dashboards: [] })
  })

  test('filters out disabled and falsy entries', () => {
    expect(
      normalizeDashComponents([
        null,
        undefined,
        false,
        { id: 'dash1', enabled: true, pos: 1 },
        { id: 'dash2', enabled: false, pos: 2 },
        { id: 'dash3', enabled: 1, pos: 3 },
        { id: 'dash4', enabled: 0, pos: 4 }
      ])
    ).toEqual({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash3', pos: 2 }
      ]
    })
  })

  test('sorts by position and normalizes positions to 1..n', () => {
    expect(
      normalizeDashComponents([
        { id: 'dash3', enabled: true, pos: 30 },
        { id: 'dash1', enabled: true, pos: 10 },
        { id: 'dash2', enabled: true, pos: 20 }
      ])
    ).toEqual({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash2', pos: 2 },
        { id: 'dash3', pos: 3 }
      ]
    })
  })

  test('rounds finite positions and sends invalid positions to the end', () => {
    expect(
      normalizeDashComponents([
        { id: 'dash1', enabled: true, pos: 2.2 },
        { id: 'dash2', enabled: true, pos: Number.NaN },
        { id: 'dash3', enabled: true, pos: 1.6 },
        { id: 'dash4', enabled: true, pos: Infinity }
      ])
    ).toEqual({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash3', pos: 2 },
        { id: 'dash2', pos: 3 },
        { id: 'dash4', pos: 4 }
      ]
    })
  })

  test('preserves stable order for equal rounded positions', () => {
    expect(
      normalizeDashComponents([
        { id: 'dash1', enabled: true, pos: 1.2 },
        { id: 'dash2', enabled: true, pos: 1.4 },
        { id: 'dash3', enabled: true, pos: 2.4 }
      ])
    ).toEqual({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash2', pos: 2 },
        { id: 'dash3', pos: 3 }
      ]
    })
  })
})
