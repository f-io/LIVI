import { renderHook, act } from '@testing-library/react'
import { useOptimisticPlaying } from '../../hooks'

jest.useFakeTimers()

describe('useOptimisticPlaying', () => {
  beforeEach(() => {
    jest.clearAllTimers()
    jest.clearAllMocks()
  })

  it('returns realPlaying when no override is set', () => {
    const { result, rerender } = renderHook(({ playing }) => useOptimisticPlaying(playing), {
      initialProps: { playing: true }
    })

    expect(result.current.uiPlaying).toBe(true)

    rerender({ playing: false })
    expect(result.current.uiPlaying).toBe(false)
  })

  it('uses override when set and clears it after timeout', async () => {
    const { result } = renderHook(() => useOptimisticPlaying(false))

    // Manually set override
    act(() => {
      result.current.setOverride(true)
    })

    // Immediately reflects override
    expect(result.current.uiPlaying).toBe(true)

    // Advance fake timers beyond 1500ms to trigger auto clear
    await act(async () => {
      jest.advanceTimersByTime(1600)
    })

    // Should now fall back to realPlaying
    expect(result.current.uiPlaying).toBe(false)
  })

  it('clears override manually when clearOverride is called', () => {
    const { result } = renderHook(() => useOptimisticPlaying(true))

    act(() => {
      result.current.setOverride(false)
    })

    expect(result.current.uiPlaying).toBe(false)

    act(() => {
      result.current.clearOverride()
    })

    expect(result.current.uiPlaying).toBe(true)
  })

  it('resets override early if realPlaying matches it', () => {
    const { result, rerender } = renderHook(({ playing }) => useOptimisticPlaying(playing), {
      initialProps: { playing: false }
    })

    act(() => {
      result.current.setOverride(true)
    })
    expect(result.current.uiPlaying).toBe(true)

    // When realPlaying updates to match override, override should clear
    rerender({ playing: true })
    expect(result.current.uiPlaying).toBe(true)
  })

  it('clears timers on unmount', () => {
    const clearSpy = jest.spyOn(window, 'clearTimeout')
    const { result, unmount } = renderHook(() => useOptimisticPlaying(false))

    act(() => {
      result.current.setOverride(true)
    })

    unmount()
    expect(clearSpy).toHaveBeenCalled()
  })
})
