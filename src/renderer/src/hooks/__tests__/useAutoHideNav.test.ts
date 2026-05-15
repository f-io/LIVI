import { act, renderHook } from '@testing-library/react'
import { useAutoHideNav } from '../useAutoHideNav'

jest.mock('../../constants', () => ({
  UI: { INACTIVITY_HIDE_DELAY_MS: 1000 }
}))

beforeEach(() => {
  jest.useFakeTimers()
})
afterEach(() => {
  jest.useRealTimers()
})

describe('useAutoHideNav', () => {
  test('disabled keeps hidden=false and never schedules a hide', () => {
    const { result } = renderHook(() => useAutoHideNav(false))
    expect(result.current.hidden).toBe(false)
    act(() => {
      jest.advanceTimersByTime(5_000)
    })
    expect(result.current.hidden).toBe(false)
  })

  test('enabled hides after the inactivity delay', () => {
    const { result } = renderHook(() => useAutoHideNav(true))
    expect(result.current.hidden).toBe(false)
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
  })

  test('wake() re-shows and re-schedules the hide', () => {
    const { result } = renderHook(() => useAutoHideNav(true))
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
    act(() => {
      result.current.wake()
    })
    expect(result.current.hidden).toBe(false)
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
  })

  test('input activity (mousemove / keydown / wheel) wakes the nav', () => {
    const { result } = renderHook(() => useAutoHideNav(true))
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
    act(() => {
      document.dispatchEvent(new MouseEvent('mousemove'))
    })
    expect(result.current.hidden).toBe(false)
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown'))
    })
    expect(result.current.hidden).toBe(false)
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
    act(() => {
      document.dispatchEvent(new WheelEvent('wheel'))
    })
    expect(result.current.hidden).toBe(false)
  })

  test('focusin without containerEl wakes', () => {
    const { result } = renderHook(() => useAutoHideNav(true))
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)
    act(() => {
      document.dispatchEvent(new FocusEvent('focusin'))
    })
    expect(result.current.hidden).toBe(false)
  })

  test('focusin only wakes when active element is inside containerEl', () => {
    const container = document.createElement('div')
    const inside = document.createElement('button')
    container.appendChild(inside)
    document.body.appendChild(container)

    const outside = document.createElement('button')
    document.body.appendChild(outside)

    const { result } = renderHook(() => useAutoHideNav(true, container))
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)

    // Focus outside → no wake
    outside.focus()
    act(() => {
      document.dispatchEvent(new FocusEvent('focusin'))
    })
    expect(result.current.hidden).toBe(true)

    // Focus inside → wake
    inside.focus()
    act(() => {
      document.dispatchEvent(new FocusEvent('focusin'))
    })
    expect(result.current.hidden).toBe(false)

    container.remove()
    outside.remove()
  })

  test('flipping enabled false clears state and detaches listeners', () => {
    const { result, rerender } = renderHook(({ on }) => useAutoHideNav(on), {
      initialProps: { on: true }
    })
    act(() => {
      jest.advanceTimersByTime(1_000)
    })
    expect(result.current.hidden).toBe(true)

    rerender({ on: false })
    expect(result.current.hidden).toBe(false)

    // mousemove now should no longer fire wake (already false, but importantly no errors and no schedule)
    act(() => {
      document.dispatchEvent(new MouseEvent('mousemove'))
      jest.advanceTimersByTime(5_000)
    })
    expect(result.current.hidden).toBe(false)
  })

  test('unmount clears timers and listeners', () => {
    const { result, unmount } = renderHook(() => useAutoHideNav(true))
    act(() => {
      jest.advanceTimersByTime(500)
    })
    unmount()
    act(() => {
      jest.advanceTimersByTime(2_000)
    })
    // Hook is gone — `hidden` reflects the last render, which was still false
    expect(result.current.hidden).toBe(false)
  })
})
