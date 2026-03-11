import { normalizeNavigationPayload } from '@main/services/projection/services/utils/normalizeNavigation'

describe('normalizeNavigationPayload', () => {
  test('merges existing navi with parsed rawUtf8 and navi patch', () => {
    const existing = {
      metaType: 100,
      navi: { CurrentRoadName: 'Old', SpeedLimit: 50 },
      rawUtf8: '',
      error: false
    } as any

    const navMsg = {
      metaType: 101,
      rawUtf8: JSON.stringify({ SpeedLimit: 60, DestinationName: 'X' }),
      navi: { CurrentRoadName: 'New' }
    }

    const out = normalizeNavigationPayload(existing, navMsg)

    expect(out).toEqual({
      metaType: 101,
      navi: {
        CurrentRoadName: 'New',
        SpeedLimit: 60,
        DestinationName: 'X'
      },
      rawUtf8: '',
      error: false
    })
  })

  test('flushes payload only for metaType=200 and NaviStatus=0', () => {
    const existing = {
      metaType: 100,
      navi: { Keep: 'old', NaviStatus: 2 },
      rawUtf8: '',
      error: false
    } as any

    const out = normalizeNavigationPayload(existing, {
      metaType: 200,
      navi: { NaviStatus: 0, Reset: true }
    })

    expect(out.navi).toEqual({ NaviStatus: 0, Reset: true })
  })

  test('handles invalid rawUtf8 safely', () => {
    const existing = {
      metaType: 7,
      navi: { A: 1 },
      rawUtf8: '',
      error: false
    } as any

    const out = normalizeNavigationPayload(existing, {
      rawUtf8: '{broken-json',
      navi: null
    })

    expect(out.metaType).toBe(7)
    expect(out.navi).toEqual({ A: 1 })
    expect(out.error).toBe(false)
  })
})
