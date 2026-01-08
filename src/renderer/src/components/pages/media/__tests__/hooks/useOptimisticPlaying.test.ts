import { renderHook, act } from '@testing-library/react'
import { useOptimisticPlaying } from '../../hooks/'

jest.useFakeTimers()

describe('useOptimisticPlaying', () => {
  beforeEach(() => {
    jest.clearAllTimers()
    jest.clearAllMocks()
  })

  it('returns realPlaying when no override is set', () => {
    const { result, rerender } = renderHook(({ playing }) => useOptimisticPlaying(playing, null), {
      initialProps: { playing: true }
    })

    expect(result.current.uiPlaying).toBe(true)

    rerender({ playing: false })
    expect(result.current.uiPlaying).toBe(false)
  })

  it('uses override when set manually', () => {
    const { result } = renderHook(() => useOptimisticPlaying(false, null))

    act(() => {
      result.current.setOverride(true)
    })
    expect(result.current.uiPlaying).toBe(true)
  })

  it('clears override manually when clearOverride is called', () => {
    const { result } = renderHook(() => useOptimisticPlaying(true, null))

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
    const { result, rerender } = renderHook(({ playing }) => useOptimisticPlaying(playing, null), {
      initialProps: { playing: false }
    })

    act(() => {
      result.current.setOverride(true)
    })
    expect(result.current.uiPlaying).toBe(true)

    // realPlaying becomes true — matches override, should auto-clear
    rerender({ playing: true })
    expect(result.current.uiPlaying).toBe(true)
  })

  it('ignores realPlaying updates when mediaPayloadError is present and override set', async () => {
    const { result, rerender } = renderHook(
      ({ playing, error }) => useOptimisticPlaying(playing, error),
      {
        initialProps: { playing: true, error: null }
      }
    )

    act(() => {
      result.current.setOverride(false)
    })
    expect(result.current.uiPlaying).toBe(false)

    // simulate incoming error payload (should keep manual override)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    rerender({ playing: true, error: new Error('metadata missing') })
    expect(result.current.uiPlaying).toBe(false)

    // simulate clearing error — now realPlaying can sync again
    await act(async () => {
      rerender({ playing: true, error: null })
    })

    // Give React effect a tick to process the cleared error
    await act(async () => {
      await Promise.resolve()
    })

    expect(result.current.uiPlaying).toBe(false)
  })

  it('auto-clears override after timeout when no error', () => {
    const { result } = renderHook(() => useOptimisticPlaying(false, null, { timeoutMs: 1500 }))

    act(() => {
      result.current.setOverride(true)
    })
    expect(result.current.uiPlaying).toBe(true)

    act(() => {
      jest.advanceTimersByTime(1600)
    })

    // after timeout, override should clear
    expect(result.current.uiPlaying).toBe(false)
  })

  it('does not auto-clear override during mediaPayloadError', () => {
    const { result } = renderHook(() =>
      useOptimisticPlaying(false, new Error('bad payload'), { timeoutMs: 1500 })
    )

    act(() => {
      result.current.setOverride(true)
    })
    expect(result.current.uiPlaying).toBe(true)

    act(() => {
      jest.advanceTimersByTime(2000)
    })

    // even after 2s, should still respect override (since error present)
    expect(result.current.uiPlaying).toBe(true)
  })

  it('clears timers on unmount', () => {
    const clearSpy = jest.spyOn(window, 'clearTimeout')
    const { result, unmount } = renderHook(() =>
      useOptimisticPlaying(false, null, { timeoutMs: 1500 })
    )

    act(() => {
      result.current.setOverride(true)
    })

    unmount()
    expect(clearSpy).toHaveBeenCalled()
  })
})
