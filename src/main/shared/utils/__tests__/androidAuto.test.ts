import {
  aaContentArea,
  clamp,
  computeAndroidAutoDpi,
  getCurrentTimeInMs,
  matchFittingAAResolution,
  pixelAspectRatioE4
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

  test('matchFittingAAResolution 1920×1080 — exact tier match', () => {
    expect(matchFittingAAResolution({ width: 1920, height: 1080 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution 2560×1440 — exact tier match', () => {
    expect(matchFittingAAResolution({ width: 2560, height: 1440 })).toEqual({
      width: 2560,
      height: 1440
    })
  })

  test('matchFittingAAResolution 800×480 — base tier', () => {
    expect(matchFittingAAResolution({ width: 800, height: 480 })).toEqual({
      width: 800,
      height: 480
    })
  })

  test('matchFittingAAResolution 1920×1200 (16:10) keeps 1920 tier — ~11% upscale within threshold', () => {
    expect(matchFittingAAResolution({ width: 1920, height: 1200 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution 1700×600 (ultrawide) escalates from 1280 to 1920 tier', () => {
    expect(matchFittingAAResolution({ width: 1700, height: 600 })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution 1024×768 (4:3) escalates from 800 to 1280 tier', () => {
    expect(matchFittingAAResolution({ width: 1024, height: 768 })).toEqual({
      width: 1280,
      height: 720
    })
  })

  test('matchFittingAAResolution caps at largest tier when user exceeds all', () => {
    const r = matchFittingAAResolution({ width: 5000, height: 3000 })
    expect(r).toEqual({ width: 3840, height: 2160 })
  })

  test('matchFittingAAResolution h264Only caps at 1920×1080', () => {
    expect(matchFittingAAResolution({ width: 2560, height: 1440 }, { h264Only: true })).toEqual({
      width: 1920,
      height: 1080
    })
  })

  test('matchFittingAAResolution h264Only allows lower tiers when display fits', () => {
    expect(matchFittingAAResolution({ width: 1280, height: 720 }, { h264Only: true })).toEqual({
      width: 1280,
      height: 720
    })
  })

  test('pixelAspectRatioE4 — matching AR returns 10000 (square pixels)', () => {
    expect(pixelAspectRatioE4({ width: 1920, height: 1080 }, { width: 1920, height: 1080 })).toBe(
      10000
    )
    expect(pixelAspectRatioE4({ width: 1280, height: 720 }, { width: 2560, height: 1440 })).toBe(
      10000
    )
  })

  test('pixelAspectRatioE4 — 16:10 display in 16:9 frame: H.264 SAR convention', () => {
    expect(pixelAspectRatioE4({ width: 1920, height: 1200 }, { width: 1920, height: 1080 })).toBe(
      9000
    )
  })

  test('pixelAspectRatioE4 — ultrawide display in 16:9 frame', () => {
    expect(pixelAspectRatioE4({ width: 1600, height: 600 }, { width: 1920, height: 1080 })).toBe(
      15000
    )
  })

  test('aaContentArea — 16:9 user in 16:9 frame: content equals frame', () => {
    expect(aaContentArea({ width: 1920, height: 1080 }, { width: 1920, height: 1080 })).toEqual({
      contentWidth: 1920,
      contentHeight: 1080
    })
  })

  test('aaContentArea — 16:10 user in 16:9 frame: content fills height, pillarbox L/R', () => {
    expect(aaContentArea({ width: 2560, height: 1440 }, { width: 2560, height: 1600 })).toEqual({
      contentWidth: 2304,
      contentHeight: 1440
    })
  })

  test('aaContentArea — ultrawide user in 16:9 frame: content fills width, letterbox T/B', () => {
    expect(aaContentArea({ width: 1920, height: 1080 }, { width: 1600, height: 600 })).toEqual({
      contentWidth: 1920,
      contentHeight: 720
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
