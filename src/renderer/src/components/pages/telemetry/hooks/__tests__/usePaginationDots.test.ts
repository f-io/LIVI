import { act, renderHook } from '@testing-library/react'
import { usePaginationDots } from '../usePaginationDots'

describe('usePaginationDots', () => {
  test('always shows dots regardless of navbar state', () => {
    const visibleNav = renderHook(() => usePaginationDots(false))
    expect(visibleNav.result.current.showDots).toBe(true)

    const hiddenNav = renderHook(() => usePaginationDots(true))
    expect(hiddenNav.result.current.showDots).toBe(true)
  })

  test('revealDots is a no-op (kept for caller API compatibility)', () => {
    const { result } = renderHook(() => usePaginationDots(true))
    act(() => {
      result.current.revealDots()
    })
    expect(result.current.showDots).toBe(true)
  })
})
