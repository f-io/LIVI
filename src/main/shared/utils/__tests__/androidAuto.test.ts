import {
  clamp,
  computeAndroidAutoDpi,
  getCurrentTimeInMs,
  matchFittingAAResolution
} from '@main/shared/utils/androidAuto'

describe('androidAuto utils', () => {
  test('clamp returns the number when already inside the range', () => {
    expect(clamp(5, 0, 10)).toBe(5)
  })

  test('clamp clamps to the minimum bound', () => {
    expect(clamp(-5, 0, 10)).toBe(0)
  })

  test('clamp clamps to the maximum bound', () => {
    expect(clamp(15, 0, 10)).toBe(10)
  })

  test('getCurrentTimeInMs returns rounded unix time in seconds', () => {
    const nowSpy = jest.spyOn(Date, 'now').mockReturnValue(1234)

    expect(getCurrentTimeInMs()).toBe(1)

    nowSpy.mockRestore()
  })

  test('matchFittingAAResolution keeps base tier for small displays', () => {
    expect(matchFittingAAResolution({ width: 1024, height: 600 })).toEqual({
      width: 800,
      height: 468
    })
  })

  test('matchFittingAAResolution selects 1280x720 tier', () => {
    expect(matchFittingAAResolution({ width: 1280, height: 720 })).toEqual({
      width: 1280,
      height: 720
    })
  })

  test('matchFittingAAResolution selects 1920x1080 tier', () => {
    expect(matchFittingAAResolution({ width: 1920, height: 1080 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution selects 2560x1440 tier', () => {
    expect(matchFittingAAResolution({ width: 2560, height: 1440 })).toEqual({
      width: 2560,
      height: 1440
    })
  })

  test('matchFittingAAResolution selects 3840x2160 tier', () => {
    expect(matchFittingAAResolution({ width: 3840, height: 2160 })).toEqual({
      width: 3840,
      height: 2160
    })
  })

  test('matchFittingAAResolution forces even height', () => {
    const result = matchFittingAAResolution({ width: 1000, height: 777 })

    expect(result.width).toBe(800)
    expect(result.height % 2).toBe(0)
  })

  test('matchFittingAAResolution clamps derived height to tier height', () => {
    expect(matchFittingAAResolution({ width: 1280, height: 2000 })).toEqual({
      width: 1280,
      height: 720
    })
  })

  test('computeAndroidAutoDpi returns minimum dpi at or below 800x480', () => {
    expect(computeAndroidAutoDpi(800, 480)).toBe(140)
    expect(computeAndroidAutoDpi(640, 360)).toBe(140)
  })

  test('computeAndroidAutoDpi returns maximum interpolated range capped at 3840x2160', () => {
    expect(computeAndroidAutoDpi(3840, 2160)).toBe(420)
    expect(computeAndroidAutoDpi(7680, 4320)).toBe(420)
  })

  test('computeAndroidAutoDpi rounds to the nearest 10', () => {
    const dpi = computeAndroidAutoDpi(1920, 1080)

    expect(dpi % 10).toBe(0)
    expect(dpi).toBeGreaterThan(140)
    expect(dpi).toBeLessThan(420)
  })
})
