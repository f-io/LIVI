import { themeColors } from '@renderer/themeColors'
import { defaultColorForPath, hexToHsl, hslToHex } from '../colorUtils'

describe('defaultColorForPath', () => {
  test.each([
    ['primaryColorDark', themeColors.primaryColorDark],
    ['primaryColorLight', themeColors.primaryColorLight],
    ['highlightColorDark', themeColors.highlightColorDark],
    ['highlightColorLight', themeColors.highlightColorLight],
    ['backgroundColorDark', themeColors.dark],
    ['backgroundColorLight', themeColors.light]
  ])('maps %s to its theme color', (path, expected) => {
    expect(defaultColorForPath(path)).toBe(expected)
  })

  test('falls back to highlightColorDark for unknown or missing paths', () => {
    expect(defaultColorForPath('nope')).toBe(themeColors.highlightColorDark)
    expect(defaultColorForPath()).toBe(themeColors.highlightColorDark)
  })
})

describe('hexToHsl', () => {
  test('converts primary colors', () => {
    expect(hexToHsl('#ff0000')).toEqual({ h: 0, s: 100, l: 50 })
    expect(hexToHsl('#00ff00')).toEqual({ h: 120, s: 100, l: 50 })
    expect(hexToHsl('#0000ff')).toEqual({ h: 240, s: 100, l: 50 })
    expect(hexToHsl('#00ffff')).toEqual({ h: 180, s: 100, l: 50 })
  })

  test('black and white have no saturation', () => {
    expect(hexToHsl('#000000')).toEqual({ h: 0, s: 0, l: 0 })
    expect(hexToHsl('#ffffff')).toEqual({ h: 0, s: 0, l: 100 })
  })

  test('accepts hex without a leading hash and trims whitespace', () => {
    expect(hexToHsl('  ff0000 ')).toEqual({ h: 0, s: 100, l: 50 })
  })

  test('returns zeroed Hsl for malformed input', () => {
    expect(hexToHsl('#zzz')).toEqual({ h: 0, s: 0, l: 0 })
    expect(hexToHsl('')).toEqual({ h: 0, s: 0, l: 0 })
  })
})

describe('hslToHex', () => {
  test('converts primary hues back to hex', () => {
    expect(hslToHex({ h: 0, s: 100, l: 50 })).toBe('#ff0000')
    expect(hslToHex({ h: 120, s: 100, l: 50 })).toBe('#00ff00')
    expect(hslToHex({ h: 240, s: 100, l: 50 })).toBe('#0000ff')
  })

  test('zero saturation yields a gray', () => {
    expect(hslToHex({ h: 0, s: 0, l: 0 })).toBe('#000000')
    expect(hslToHex({ h: 0, s: 0, l: 100 })).toBe('#ffffff')
  })
})

describe('hex/hsl round-trip', () => {
  test.each(['#ff0000', '#00ff00', '#0000ff', '#00ffff', '#ff00ff', '#ffff00'])(
    'preserves %s through hexToHsl → hslToHex',
    (hex) => {
      expect(hslToHex(hexToHsl(hex))).toBe(hex)
    }
  )
})
