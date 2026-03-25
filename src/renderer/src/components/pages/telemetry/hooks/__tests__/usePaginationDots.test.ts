import { act, renderHook } from '@testing-library/react'
import { usePaginationDots } from '../usePaginationDots'

describe('usePaginationDots', () => {
  beforeEach(() => {
    jest.useFakeTimers()
    jest.clearAllMocks()
  })

  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
  })

  test('shows dots immediately when navbar is visible', () => {
    const { result } = renderHook(() => usePaginationDots(false))

    expect(result.current.showDots).toBe(true)
  })

  test('hides dots immediately when navbar is hidden', () => {
    const { result } = renderHook(() => usePaginationDots(true))

    expect(result.current.showDots).toBe(false)
  })

  test('revealDots shows dots and hides them again after 2000ms when navbar is hidden', () => {
    const { result } = renderHook(() => usePaginationDots(true))

    expect(result.current.showDots).toBe(false)

    act(() => {
      result.current.revealDots()
    })

    expect(result.current.showDots).toBe(true)

    act(() => {
      jest.advanceTimersByTime(1999)
    })

    expect(result.current.showDots).toBe(true)

    act(() => {
      jest.advanceTimersByTime(1)
    })

    expect(result.current.showDots).toBe(false)
  })

  test('revealDots resets the hide timer when called multiple times', () => {
    const { result } = renderHook(() => usePaginationDots(true))

    act(() => {
      result.current.revealDots()
    })

    act(() => {
      jest.advanceTimersByTime(1500)
    })

    expect(result.current.showDots).toBe(true)

    act(() => {
      result.current.revealDots()
    })

    act(() => {
      jest.advanceTimersByTime(1500)
    })

    expect(result.current.showDots).toBe(true)

    act(() => {
      jest.advanceTimersByTime(500)
    })

    expect(result.current.showDots).toBe(false)
  })

  test('switching navbar from hidden to visible shows dots and clears pending timer', () => {
    const { result, rerender } = renderHook(
      ({ isNavbarHidden }) => usePaginationDots(isNavbarHidden),
      { initialProps: { isNavbarHidden: true } }
    )

    act(() => {
      result.current.revealDots()
    })

    expect(result.current.showDots).toBe(true)

    rerender({ isNavbarHidden: false })

    expect(result.current.showDots).toBe(true)

    act(() => {
      jest.advanceTimersByTime(3000)
    })

    expect(result.current.showDots).toBe(true)
  })

  test('switching navbar from visible to hidden hides dots immediately', () => {
    const { result, rerender } = renderHook(
      ({ isNavbarHidden }) => usePaginationDots(isNavbarHidden),
      { initialProps: { isNavbarHidden: false } }
    )

    expect(result.current.showDots).toBe(true)

    rerender({ isNavbarHidden: true })

    expect(result.current.showDots).toBe(false)
  })

  test('clears timeout on unmount', () => {
    const clearTimeoutSpy = jest.spyOn(window, 'clearTimeout')

    const { result, unmount } = renderHook(() => usePaginationDots(true))

    act(() => {
      result.current.revealDots()
    })

    unmount()

    expect(clearTimeoutSpy).toHaveBeenCalled()

    clearTimeoutSpy.mockRestore()
  })
})
