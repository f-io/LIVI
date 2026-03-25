import { clamp } from '../clamp'

describe('clamp', () => {
  test('returns value within range unchanged', () => {
    expect(clamp(5, 0, 10)).toBe(5)
  })

  test('clamps value below min', () => {
    expect(clamp(-5, 0, 10)).toBe(0)
  })

  test('clamps value above max', () => {
    expect(clamp(15, 0, 10)).toBe(10)
  })

  test('works when value equals boundaries', () => {
    expect(clamp(0, 0, 10)).toBe(0)
    expect(clamp(10, 0, 10)).toBe(10)
  })

  test('works with negative ranges', () => {
    expect(clamp(-5, -10, -1)).toBe(-5)
    expect(clamp(-15, -10, -1)).toBe(-10)
    expect(clamp(0, -10, -1)).toBe(-1)
  })
})
