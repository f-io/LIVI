import {
  clamp,
  computeAndroidAutoDpi,
  getCurrentTimeInMs,
  matchFittingAAResolution
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

  test('matchFittingAAResolution picks 1080p tier for 1920×1080', () => {
    expect(matchFittingAAResolution({ width: 1920, height: 1080 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution 1600×600 (ultrawide) — picks 1920 tier for quality', () => {
    expect(matchFittingAAResolution({ width: 1600, height: 600 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution tiny 300×200 — base tier', () => {
    expect(matchFittingAAResolution({ width: 300, height: 200 })).toEqual({
      width: 800,
      height: 480
    })
  })

  test('matchFittingAAResolution beyond max tier — caps at 3840×2160', () => {
    expect(matchFittingAAResolution({ width: 4000, height: 1000 })).toEqual({
      width: 3840,
      height: 2160
    })
  })

  test('computeAndroidAutoDpi returns minimum dpi for 800x480 and smaller', () => {
    expect(computeAndroidAutoDpi(800, 480)).toBe(140)
    expect(computeAndroidAutoDpi(400, 240)).toBe(140)
  })

  test('computeAndroidAutoDpi scales up with resolution', () => {
    expect(computeAndroidAutoDpi(1280, 720)).toBe(180)
    expect(computeAndroidAutoDpi(1920, 1080)).toBe(200)
    expect(computeAndroidAutoDpi(2560, 1440)).toBe(250)
    expect(computeAndroidAutoDpi(3840, 2160)).toBe(420)
  })

  test('computeAndroidAutoDpi clamps at maximum dpi', () => {
    expect(computeAndroidAutoDpi(5000, 3000)).toBe(420)
  })
})
