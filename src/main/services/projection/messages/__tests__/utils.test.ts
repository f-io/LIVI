import {
  AA_ALLOWED,
  clamp,
  getCurrentTimeInMs,
  matchFittingAAResolution
} from '@main/services/projection/messages/utils'

describe('projection message utils', () => {
  test('clamp limits values to inclusive range', () => {
    expect(clamp(5, 0, 10)).toBe(5)
    expect(clamp(-1, 0, 10)).toBe(0)
    expect(clamp(99, 0, 10)).toBe(10)
  })

  test('getCurrentTimeInMs returns seconds from Date.now rounded', () => {
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(1234)
    expect(getCurrentTimeInMs()).toBe(1)
    nowSpy.mockRestore()
  })

  test('AA_ALLOWED contains expected resolution tiers', () => {
    expect(AA_ALLOWED[0]).toEqual({ width: 800, height: 480 })
    expect(AA_ALLOWED[AA_ALLOWED.length - 1]).toEqual({ width: 3840, height: 2160 })
  })

  test('matchFittingAAResolution picks highest tier that fits (rotation-safe)', () => {
    expect(matchFittingAAResolution({ width: 1920, height: 1080 })).toEqual({
      width: 1920,
      height: 1080
    })
    expect(matchFittingAAResolution({ width: 1080, height: 1920 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution falls back to smallest tier when display too small', () => {
    expect(matchFittingAAResolution({ width: 300, height: 200 })).toEqual({
      width: 800,
      height: 480
    })
  })
})
