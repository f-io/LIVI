import {
  clamp,
  getCurrentTimeInMs,
  matchFittingAAResolution,
  computeAndroidAutoDpi
} from '@shared/utils'

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

  test('matchFittingAAResolution picks 1080p tier for 1920x1080', () => {
    expect(matchFittingAAResolution({ width: 1920, height: 1080 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution picks 720p tier and derives height from aspect ratio', () => {
    expect(matchFittingAAResolution({ width: 1600, height: 600 })).toEqual({
      width: 1280,
      height: 480
    })
  })

  test('matchFittingAAResolution falls back to smallest tier when display too small', () => {
    expect(matchFittingAAResolution({ width: 300, height: 200 })).toEqual({
      width: 800,
      height: 480
    })
  })

  test('matchFittingAAResolution keeps height even and clamps to tier height', () => {
    expect(matchFittingAAResolution({ width: 4000, height: 1000 })).toEqual({
      width: 3840,
      height: 960
    })
  })

  test('computeAndroidAutoDpi returns minimum dpi for 800x480 and smaller', () => {
    expect(computeAndroidAutoDpi(800, 480)).toBe(140)
    expect(computeAndroidAutoDpi(400, 240)).toBe(140)
  })

  test('computeAndroidAutoDpi scales up with resolution', () => {
    expect(computeAndroidAutoDpi(1280, 720)).toBe(160)
    expect(computeAndroidAutoDpi(1920, 1080)).toBe(200)
    expect(computeAndroidAutoDpi(2560, 1440)).toBe(260)
    expect(computeAndroidAutoDpi(3840, 2160)).toBe(420)
  })

  test('computeAndroidAutoDpi clamps at maximum dpi', () => {
    expect(computeAndroidAutoDpi(5000, 3000)).toBe(420)
  })
})
