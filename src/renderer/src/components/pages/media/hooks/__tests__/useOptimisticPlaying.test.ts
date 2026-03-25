import { act, renderHook } from '@testing-library/react'
import { useOptimisticPlaying, useOptimisticPlaying_deprecated } from '../useOptimisticPlaying'

describe('useOptimisticPlaying_deprecated', () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
  })

  test('uses realPlaying when no override is set', () => {
    const { result, rerender } = renderHook(
      ({ realPlaying }) => useOptimisticPlaying_deprecated(realPlaying),
      {
        initialProps: { realPlaying: true as boolean | undefined }
      }
    )

    expect(result.current.uiPlaying).toBe(true)

    rerender({ realPlaying: false })

    expect(result.current.uiPlaying).toBe(false)
  })

  test('uses override until realPlaying matches and then clears it', () => {
    const { result, rerender } = renderHook(
      ({ realPlaying }) => useOptimisticPlaying_deprecated(realPlaying),
      {
        initialProps: { realPlaying: false as boolean | undefined }
      }
    )

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)

    rerender({ realPlaying: true })

    expect(result.current.uiPlaying).toBe(true)
  })

  test('auto clears override after 1500ms', () => {
    const { result } = renderHook(() => useOptimisticPlaying_deprecated(false))

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)

    act(() => {
      jest.advanceTimersByTime(1500)
    })

    expect(result.current.uiPlaying).toBe(false)
  })

  test('clearOverride resets override immediately', () => {
    const { result } = renderHook(() => useOptimisticPlaying_deprecated(false))

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)

    act(() => {
      result.current.clearOverride()
    })

    expect(result.current.uiPlaying).toBe(false)
  })
})

describe('useOptimisticPlaying', () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
  })

  test('uses realPlaying when override is null', () => {
    const { result, rerender } = renderHook(
      ({ realPlaying, mediaPayloadError }) => useOptimisticPlaying(realPlaying, mediaPayloadError),
      {
        initialProps: {
          realPlaying: true as boolean | undefined,
          mediaPayloadError: null as unknown
        }
      }
    )

    expect(result.current.uiPlaying).toBe(true)

    rerender({ realPlaying: false, mediaPayloadError: null })

    expect(result.current.uiPlaying).toBe(false)
  })

  test('setOverride marks manual override and updates uiPlaying', () => {
    const { result } = renderHook(() => useOptimisticPlaying(false, null))

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)
    expect(result.current._internal.manualRef.current).toBe(true)
  })

  test('clearOverride clears manual override and timer', () => {
    const { result } = renderHook(() => useOptimisticPlaying(false, null, { timeoutMs: 500 }))

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)
    expect(result.current._internal.manualRef.current).toBe(true)

    act(() => {
      result.current.clearOverride()
    })

    expect(result.current.uiPlaying).toBe(false)
    expect(result.current._internal.manualRef.current).toBe(false)
  })

  test('auto clears override after timeout when timeoutMs is provided and there is no error', () => {
    const { result } = renderHook(() => useOptimisticPlaying(false, null, { timeoutMs: 500 }))

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)

    act(() => {
      jest.advanceTimersByTime(500)
    })

    expect(result.current.uiPlaying).toBe(false)
    expect(result.current._internal.manualRef.current).toBe(false)
  })

  test('does not start timeout when mediaPayloadError exists', () => {
    const { result } = renderHook(() =>
      useOptimisticPlaying(false, new Error('payload failed'), { timeoutMs: 500 })
    )

    act(() => {
      result.current.setOverride(true)
    })

    act(() => {
      jest.advanceTimersByTime(500)
    })

    expect(result.current.uiPlaying).toBe(true)
    expect(result.current._internal.manualRef.current).toBe(true)
  })

  test('keeps manual override when error exists and realPlaying changes', () => {
    const { result, rerender } = renderHook(
      ({ realPlaying, mediaPayloadError }) => useOptimisticPlaying(realPlaying, mediaPayloadError),
      {
        initialProps: {
          realPlaying: false as boolean | undefined,
          mediaPayloadError: null as unknown
        }
      }
    )

    act(() => {
      result.current.setOverride(true)
    })

    rerender({
      realPlaying: false,
      mediaPayloadError: new Error('payload error')
    })

    expect(result.current.uiPlaying).toBe(true)
    expect(result.current._internal.manualRef.current).toBe(true)
  })

  test('clears override when realPlaying matches override and there is no error', () => {
    const { result, rerender } = renderHook(
      ({ realPlaying, mediaPayloadError }) =>
        useOptimisticPlaying(realPlaying, mediaPayloadError, { timeoutMs: 500 }),
      {
        initialProps: {
          realPlaying: false as boolean | undefined,
          mediaPayloadError: null as unknown
        }
      }
    )

    act(() => {
      result.current.setOverride(true)
    })

    expect(result.current.uiPlaying).toBe(true)

    rerender({
      realPlaying: true,
      mediaPayloadError: null
    })

    expect(result.current.uiPlaying).toBe(true)
    expect(result.current._internal.manualRef.current).toBe(false)
  })

  test('clears stale override when manualRef is false and override is still set', () => {
    const { result, rerender } = renderHook(
      ({ realPlaying, mediaPayloadError }) => useOptimisticPlaying(realPlaying, mediaPayloadError),
      {
        initialProps: {
          realPlaying: false as boolean | undefined,
          mediaPayloadError: null as unknown
        }
      }
    )

    act(() => {
      result.current.setOverride(true)
    })

    act(() => {
      result.current.clearOverride()
    })

    rerender({
      realPlaying: false,
      mediaPayloadError: null
    })

    expect(result.current.uiPlaying).toBe(false)
    expect(result.current._internal.manualRef.current).toBe(false)
  })

  test('uses false when both override and realPlaying are falsy', () => {
    const { result } = renderHook(() => useOptimisticPlaying(undefined, null))

    expect(result.current.uiPlaying).toBe(false)
  })

  test('cleans up timer on unmount', () => {
    const clearTimeoutSpy = jest.spyOn(window, 'clearTimeout')

    const { result, unmount } = renderHook(() =>
      useOptimisticPlaying(false, null, { timeoutMs: 500 })
    )

    act(() => {
      result.current.setOverride(true)
    })

    unmount()

    expect(clearTimeoutSpy).toHaveBeenCalled()

    clearTimeoutSpy.mockRestore()
  })
})
