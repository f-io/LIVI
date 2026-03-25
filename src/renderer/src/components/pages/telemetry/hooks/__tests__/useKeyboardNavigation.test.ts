import { act, renderHook } from '@testing-library/react'
import { useKeyboardNavigation } from '../useKeyboardNavigation'

const revealDotsMock = jest.fn()

jest.mock('@renderer/components/pages/telemetry/hooks/usePaginationDots', () => ({
  usePaginationDots: () => ({
    revealDots: revealDotsMock
  })
}))

describe('useKeyboardNavigation', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('prev navigates to previous dashboard when possible', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.prev()
    })

    expect(onSetIndex).toHaveBeenCalledTimes(1)

    const updater = onSetIndex.mock.calls[0][0]
    expect(updater(1)).toBe(0)
    expect(revealDotsMock).toHaveBeenCalledTimes(1)
  })

  test('prev does nothing on first dashboard', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.prev()
    })

    expect(onSetIndex).not.toHaveBeenCalled()
    expect(revealDotsMock).not.toHaveBeenCalled()
  })

  test('next navigates to next dashboard when possible', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.next()
    })

    expect(onSetIndex).toHaveBeenCalledTimes(1)

    const updater = onSetIndex.mock.calls[0][0]
    expect(updater(1)).toBe(2)
    expect(revealDotsMock).toHaveBeenCalledTimes(1)
  })

  test('next does nothing on last dashboard', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.next()
    })

    expect(onSetIndex).not.toHaveBeenCalled()
    expect(revealDotsMock).not.toHaveBeenCalled()
  })

  test('next does nothing when dashboard list is empty', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.next()
    })

    expect(onSetIndex).not.toHaveBeenCalled()
  })

  test('canPrev and canNext reflect current pager state', () => {
    const onSetIndex = jest.fn()

    const { result, rerender } = renderHook(
      ({ dashboards, index }) =>
        useKeyboardNavigation({
          dashboards,
          index,
          isNavbarHidden: true,
          onSetIndex
        }),
      {
        initialProps: {
          dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
          index: 0
        }
      }
    )

    expect(result.current.canPrev()).toBe(false)
    expect(result.current.canNext()).toBe(true)

    rerender({
      dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
      index: 1
    })

    expect(result.current.canPrev()).toBe(true)
    expect(result.current.canNext()).toBe(true)

    rerender({
      dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
      index: 2
    })

    expect(result.current.canPrev()).toBe(true)
    expect(result.current.canNext()).toBe(false)
  })

  test('onNavigate clamps index and reveals dots', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onNavigate(1)
    })

    let updater = onSetIndex.mock.calls[0][0]
    expect(updater(1)).toBe(2)

    act(() => {
      result.current.onNavigate(-1)
    })

    updater = onSetIndex.mock.calls[1][0]
    expect(updater(1)).toBe(0)

    expect(revealDotsMock).toHaveBeenCalledTimes(2)
  })

  test('onNavigate does nothing when there is only one dashboard', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onNavigate(1)
    })

    expect(onSetIndex).not.toHaveBeenCalled()
    expect(revealDotsMock).not.toHaveBeenCalled()
  })

  test('onPointerDown ignores non-primary pointer', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: false,
        clientX: 100,
        clientY: 50
      } as React.PointerEvent)
    })

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 0,
        clientY: 50
      } as React.PointerEvent)
    })

    expect(onSetIndex).not.toHaveBeenCalled()
  })

  test('swipe left navigates to next dashboard', () => {
    const onSetIndex = jest.fn()
    const performanceSpy = jest
      .spyOn(performance, 'now')
      .mockReturnValueOnce(1000)
      .mockReturnValueOnce(1200)

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: true,
        clientX: 200,
        clientY: 100
      } as React.PointerEvent)
    })

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 120,
        clientY: 110
      } as React.PointerEvent)
    })

    expect(onSetIndex).toHaveBeenCalledTimes(1)
    const updater = onSetIndex.mock.calls[0][0]
    expect(updater(1)).toBe(2)

    performanceSpy.mockRestore()
  })

  test('swipe right navigates to previous dashboard', () => {
    const onSetIndex = jest.fn()
    const performanceSpy = jest
      .spyOn(performance, 'now')
      .mockReturnValueOnce(1000)
      .mockReturnValueOnce(1200)

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: true,
        clientX: 100,
        clientY: 100
      } as React.PointerEvent)
    })

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 180,
        clientY: 110
      } as React.PointerEvent)
    })

    expect(onSetIndex).toHaveBeenCalledTimes(1)
    const updater = onSetIndex.mock.calls[0][0]
    expect(updater(1)).toBe(0)

    performanceSpy.mockRestore()
  })

  test('pointer up ignores non-primary pointer', () => {
    const onSetIndex = jest.fn()

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: true,
        clientX: 100,
        clientY: 100
      } as React.PointerEvent)
    })

    act(() => {
      result.current.onPointerUp({
        isPrimary: false,
        clientX: 0,
        clientY: 100
      } as React.PointerEvent)
    })

    expect(onSetIndex).not.toHaveBeenCalled()
  })

  test('pointer up does nothing when there was no pointer down', () => {
    const onSetIndex = jest.fn()
    const performanceSpy = jest.spyOn(performance, 'now').mockReturnValue(1200)

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 0,
        clientY: 100
      } as React.PointerEvent)
    })

    expect(onSetIndex).not.toHaveBeenCalled()

    performanceSpy.mockRestore()
  })

  test('pointer up ignores swipe when horizontal movement is too small', () => {
    const onSetIndex = jest.fn()
    const performanceSpy = jest
      .spyOn(performance, 'now')
      .mockReturnValueOnce(1000)
      .mockReturnValueOnce(1200)

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }],
        index: 0,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: true,
        clientX: 100,
        clientY: 100
      } as React.PointerEvent)
    })

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 70,
        clientY: 100
      } as React.PointerEvent)
    })

    expect(onSetIndex).not.toHaveBeenCalled()

    performanceSpy.mockRestore()
  })

  test('pointer up ignores swipe when vertical movement dominates', () => {
    const onSetIndex = jest.fn()
    const performanceSpy = jest
      .spyOn(performance, 'now')
      .mockReturnValueOnce(1000)
      .mockReturnValueOnce(1200)

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: true,
        clientX: 200,
        clientY: 100
      } as React.PointerEvent)
    })

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 140,
        clientY: 180
      } as React.PointerEvent)
    })

    expect(onSetIndex).not.toHaveBeenCalled()

    performanceSpy.mockRestore()
  })

  test('pointer up ignores swipe when gesture is too slow', () => {
    const onSetIndex = jest.fn()
    let now = 1000

    const performanceSpy = jest.spyOn(performance, 'now').mockImplementation(() => now)

    const { result } = renderHook(() =>
      useKeyboardNavigation({
        dashboards: [{ id: 'a' }, { id: 'b' }, { id: 'c' }],
        index: 1,
        isNavbarHidden: true,
        onSetIndex
      })
    )

    act(() => {
      result.current.onPointerDown({
        isPrimary: true,
        clientX: 200,
        clientY: 100
      } as React.PointerEvent)
    })

    now = 2001

    act(() => {
      result.current.onPointerUp({
        isPrimary: true,
        clientX: 120,
        clientY: 100
      } as React.PointerEvent)
    })

    expect(onSetIndex).not.toHaveBeenCalled()

    performanceSpy.mockRestore()
  })
})
