import { act, renderHook } from '@testing-library/react'
import { useBlinkingTime } from '../useBlinkingTime'

describe('useBlinkingTime', () => {
  test('returns time string and updates every second', () => {
    jest.useFakeTimers()
    const { result } = renderHook(() => useBlinkingTime())
    const first = result.current

    act(() => {
      jest.advanceTimersByTime(1000)
    })
    const second = result.current
    expect(typeof first).toBe('string')
    expect(typeof second).toBe('string')
    jest.useRealTimers()
  })
})
