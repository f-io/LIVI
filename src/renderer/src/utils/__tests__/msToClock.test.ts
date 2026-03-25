import { msToClock } from '../msToClock'

describe('msToClock', () => {
  test('returns 0:00 for undefined', () => {
    expect(msToClock()).toBe('0:00')
  })

  test('returns 0:00 for 0', () => {
    expect(msToClock(0)).toBe('0:00')
  })

  test('returns 0:00 for negative values', () => {
    expect(msToClock(-1)).toBe('0:00')
    expect(msToClock(-5000)).toBe('0:00')
  })

  test('formats seconds below 10 with leading zero', () => {
    expect(msToClock(1000)).toBe('0:01')
    expect(msToClock(9000)).toBe('0:09')
  })

  test('formats full minutes correctly', () => {
    expect(msToClock(60000)).toBe('1:00')
    expect(msToClock(61000)).toBe('1:01')
    expect(msToClock(125000)).toBe('2:05')
  })

  test('floors milliseconds to whole seconds', () => {
    expect(msToClock(1999)).toBe('0:01')
    expect(msToClock(59999)).toBe('0:59')
  })
})
