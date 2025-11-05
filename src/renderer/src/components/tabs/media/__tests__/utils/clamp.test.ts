import { clamp } from '../../utils'

describe('clamp', () => {
  it('returns a number if it is within the range', () => {
    expect(clamp(5, 0, 10)).toBe(5)
  })

  it('limits the number from below if it is less than min', () => {
    expect(clamp(-3, 0, 10)).toBe(0)
  })

  it('ограничивает число сверху, если больше max', () => {
    expect(clamp(15, 0, 10)).toBe(10)
  })

  it('limits the number from above if it is greater than max', () => {
    expect(clamp(5, 5, 5)).toBe(5)
    expect(clamp(0, 5, 5)).toBe(5)
    expect(clamp(10, 5, 5)).toBe(5)
  })

  it('handles negative values correctly', () => {
    expect(clamp(-5, -10, -1)).toBe(-5)
    expect(clamp(-15, -10, -1)).toBe(-10)
    expect(clamp(0, -10, -1)).toBe(-1)
  })
})
