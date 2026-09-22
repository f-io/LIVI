import { act, renderHook } from '@testing-library/react'
import type { Mock } from 'vitest'
import { useMediaState } from '../../hooks'
import { clamp, mergePayload, payloadFromLiveEvent } from '../../utils'

vi.mock('../../utils/clamp', () => ({
  clamp: vi.fn((n: number, min: number, max: number) => Math.max(min, Math.min(max, n)))
}))
vi.mock('../../utils/mergePayload', () => ({
  mergePayload: vi.fn()
}))
vi.mock('../../utils/payloadFromLiveEvent', () => ({
  payloadFromLiveEvent: vi.fn()
}))

const mockReadMedia = vi.fn()
const mockOnEvent = vi.fn()
const mockRemoveListener = vi.fn()

beforeEach(async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true })

  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
    setTimeout(() => cb(performance.now()), 16)
    return 1
  })
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {})
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-expect-error
  ;(window as never).projection = {
    ipc: {
      readMedia: mockReadMedia,
      onEvent: mockOnEvent
    }
  }
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-expect-error
  ;(window as never).electron = {
    ipcRenderer: {
      removeListener: mockRemoveListener
    }
  }

  mockReadMedia.mockReset()
  mockOnEvent.mockReset()
  mockRemoveListener.mockReset()
  ;(clamp as Mock).mockClear()
  ;(mergePayload as Mock).mockClear()
  ;(payloadFromLiveEvent as Mock).mockClear()
})

afterEach(async () => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('useMediaState', () => {
  it('returns initial state', async () => {
    const { result } = renderHook(() => useMediaState(false))
    expect(result.current.snap).toBeNull()
    expect(result.current.livePlayMs).toBe(0)
  })

  it('hydrates initial state if allowed', async () => {
    const mockPayload = {
      payload: {
        type: 1,
        media: { MediaSongPlayTime: 123 }
      }
    }
    mockReadMedia.mockResolvedValueOnce(mockPayload)

    const { result } = renderHook(() => useMediaState(true))

    await act(async () => {
      await Promise.resolve()
    })

    expect(mockReadMedia).toHaveBeenCalled()
    expect(result.current.snap).toEqual(mockPayload)
    expect(result.current.livePlayMs).toBe(123)
  })

  it('subscribes to projection events and unsubscribes on unmount', async () => {
    const unsubscribe = vi.fn()
    mockOnEvent.mockReturnValueOnce(unsubscribe)

    const { unmount } = renderHook(() => useMediaState(false))

    expect(mockOnEvent).toHaveBeenCalled()
    unmount()
    expect(unsubscribe).toHaveBeenCalled()
  })

  it('falls back to electron.removeListener when unsubscribe not provided', async () => {
    mockOnEvent.mockReturnValueOnce(undefined)

    const { unmount } = renderHook(() => useMediaState(false))
    unmount()

    expect(mockRemoveListener).toHaveBeenCalledWith('projection-event', expect.any(Function))
  })

  it('handles unplugged event correctly', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'unplugged' })
    })

    expect(result.current.snap).toBeNull()
    expect(result.current.livePlayMs).toBe(0)
  })

  it('handles valid live event and updates state', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })

    const inc = {
      type: 1,
      media: { MediaSongPlayTime: 50 }
    }
    const merged = {
      type: 1,
      media: { MediaSongPlayTime: 50 }
    }

    ;(payloadFromLiveEvent as Mock).mockReturnValue(inc)
    ;(mergePayload as Mock).mockReturnValue(merged)

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'media', payload: { payload: inc } })
    })

    expect(payloadFromLiveEvent).toHaveBeenCalled()
    expect(mergePayload).toHaveBeenCalled()
    expect(result.current.livePlayMs).toBe(50)
    expect(result.current.snap?.payload.media?.MediaSongPlayTime).toBe(50)
  })

  it('does not call mergePayload when payloadFromLiveEvent returns null', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    ;(payloadFromLiveEvent as Mock).mockReturnValue(null)

    renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'media', payload: { payload: null } })
    })

    expect(payloadFromLiveEvent).toHaveBeenCalled()
    expect(mergePayload).not.toHaveBeenCalled()
  })

  it('updates playback time in animation loop when playing', async () => {
    const payload = {
      timestamp: '2025-01-01T00:00:00Z',
      payload: {
        type: 1,
        media: { MediaPlayStatus: 1, MediaSongDuration: 1000, MediaSongPlayTime: 10 }
      }
    }
    mockReadMedia.mockResolvedValueOnce(payload)
    const { result } = renderHook(() => useMediaState(true))

    // wait for hydration
    await act(async () => {
      await Promise.resolve()
    })

    expect(result.current.snap).toEqual(payload)
    expect(result.current.livePlayMs).toBe(10)

    // advance multiple frames — await act each iteration to avoid warning
    for (let i = 0; i < 5; i++) {
      await act(async () => {
        vi.advanceTimersByTime(50)
        await Promise.resolve()
      })
    }

    expect(clamp).toHaveBeenCalled()
  })

  it('does not update playback time when paused', async () => {
    const payloadPaused = {
      timestamp: '2025-01-01T00:00:00Z',
      payload: {
        type: 1,
        media: { MediaPlayStatus: 2, MediaSongDuration: 1000, MediaSongPlayTime: 10 }
      }
    }
    mockReadMedia.mockResolvedValueOnce(payloadPaused)
    const { result } = renderHook(() => useMediaState(true))

    await act(async () => {
      await Promise.resolve()
    })

    expect(result.current.snap).toEqual(payloadPaused)
    expect(result.current.livePlayMs).toBe(10)

    for (let i = 0; i < 5; i++) {
      await act(async () => {
        vi.advanceTimersByTime(50)
        await Promise.resolve()
      })
    }

    expect(clamp).not.toHaveBeenCalled()
  })

  it('reuses previous play time when incoming media event has no MediaSongPlayTime', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    ;(payloadFromLiveEvent as Mock)
      .mockReturnValueOnce({
        type: 1,
        media: { MediaSongPlayTime: 123 }
      })
      .mockReturnValueOnce({
        type: 1,
        media: {}
      })
    ;(mergePayload as Mock)
      .mockReturnValueOnce({
        type: 1,
        media: { MediaSongPlayTime: 123 }
      })
      .mockReturnValueOnce({
        type: 1,
        media: {}
      })

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'media', payload: { payload: { media: { MediaSongPlayTime: 123 } } } })
    })

    expect(result.current.livePlayMs).toBe(123)
    expect(result.current.snap?.payload.media?.MediaSongPlayTime).toBe(123)

    act(() => {
      handler({}, { type: 'media', payload: { payload: { media: {} } } })
    })

    expect(result.current.livePlayMs).toBe(123)
    expect(result.current.snap?.payload.media?.MediaSongPlayTime).toBeUndefined()
  })

  it('treats an event handler call with no payload as an empty event', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    ;(payloadFromLiveEvent as Mock).mockReturnValue(null)

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({})
    })

    expect(payloadFromLiveEvent).toHaveBeenCalledWith({})
    expect(result.current.snap).toBeNull()
  })

  it('skips subscription when onEvent is not a function', async () => {
    ;(window as never as { projection: { ipc: { onEvent?: unknown } } }).projection.ipc.onEvent =
      undefined

    const { unmount } = renderHook(() => useMediaState(false))
    unmount()

    expect(mockRemoveListener).toHaveBeenCalledWith('projection-event', expect.any(Function))
  })

  it('does not hydrate when readMedia resolves to nothing', async () => {
    mockReadMedia.mockResolvedValueOnce(null)

    const { result } = renderHook(() => useMediaState(true))

    await act(async () => {
      await Promise.resolve()
    })

    expect(mockReadMedia).toHaveBeenCalled()
    expect(result.current.snap).toBeNull()
    expect(result.current.livePlayMs).toBe(0)
  })

  it('hydrates with zero play time when the snapshot has no media', async () => {
    mockReadMedia.mockResolvedValueOnce({
      timestamp: '2025-01-01T00:00:00Z',
      payload: { type: 1 }
    })

    const { result } = renderHook(() => useMediaState(true))

    await act(async () => {
      await Promise.resolve()
    })

    expect(result.current.snap?.payload.type).toBe(1)
    expect(result.current.livePlayMs).toBe(0)
  })

  it('clamps against zero duration while playing without a song duration', async () => {
    const payload = {
      timestamp: '2025-01-01T00:00:00Z',
      payload: {
        type: 1,
        media: { MediaPlayStatus: 1, MediaSongPlayTime: 10 }
      }
    }
    mockReadMedia.mockResolvedValueOnce(payload)
    renderHook(() => useMediaState(true))

    await act(async () => {
      await Promise.resolve()
    })

    for (let i = 0; i < 5; i++) {
      await act(async () => {
        vi.advanceTimersByTime(50)
        await Promise.resolve()
      })
    }

    expect(clamp).toHaveBeenCalledWith(expect.any(Number), 0, 0)
  })

  it('handles media-reset by re-reading media and seeding play time', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    mockReadMedia.mockResolvedValueOnce({
      timestamp: '2025-01-01T00:00:00Z',
      payload: { type: 1, media: { MediaSongPlayTime: 321 } }
    })

    const { result } = renderHook(() => useMediaState(false))

    await act(async () => {
      handler({}, { type: 'media-reset' })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(mockReadMedia).toHaveBeenCalled()
    expect(result.current.livePlayMs).toBe(321)
    expect(result.current.snap?.payload.media?.MediaSongPlayTime).toBe(321)
  })

  it('handles media-reset with missing media by seeding play time to zero', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    mockReadMedia.mockResolvedValueOnce({
      timestamp: '2025-01-01T00:00:00Z',
      payload: { type: 1 }
    })

    const { result } = renderHook(() => useMediaState(false))

    await act(async () => {
      handler({}, { type: 'media-reset' })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(result.current.livePlayMs).toBe(0)
    expect(result.current.snap?.payload.type).toBe(1)
  })

  it('ignores media-reset when readMedia returns nothing', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    mockReadMedia.mockResolvedValueOnce(null)

    const { result } = renderHook(() => useMediaState(false))

    await act(async () => {
      handler({}, { type: 'media-reset' })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(result.current.snap).toBeNull()
    expect(result.current.livePlayMs).toBe(0)
  })

  it('swallows errors thrown while handling media-reset', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    mockReadMedia.mockRejectedValueOnce(new Error('boom'))

    const { result } = renderHook(() => useMediaState(false))

    await act(async () => {
      handler({}, { type: 'media-reset' })
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(result.current.snap).toBeNull()
  })

  it('falls back to 0 when incoming media event has no MediaSongPlayTime and previous play time is not a number', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    ;(payloadFromLiveEvent as Mock).mockReturnValue({
      type: 1,
      media: {}
    })
    ;(mergePayload as Mock).mockReturnValue({
      type: 1,
      media: {}
    })

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'media', payload: { payload: { media: {} } } })
    })

    expect(result.current.livePlayMs).toBe(0)
  })

  it('stalls and falls back to the reported play time when the phone stops reporting progress', async () => {
    mockReadMedia.mockResolvedValueOnce({
      timestamp: '2025-01-01T00:00:00Z',
      payload: {
        type: 1,
        media: { MediaPlayStatus: 1, MediaSongDuration: 600000, MediaSongPlayTime: 1000 }
      }
    })
    const { result } = renderHook(() => useMediaState(true))
    await act(async () => {
      await Promise.resolve()
    })
    expect(result.current.stalled).toBe(false)

    await act(async () => {
      vi.advanceTimersByTime(2000)
      await Promise.resolve()
    })
    expect(result.current.stalled).toBe(false)
    expect(result.current.livePlayMs).toBeGreaterThan(1000)

    await act(async () => {
      vi.advanceTimersByTime(1500)
      await Promise.resolve()
    })
    expect(result.current.stalled).toBe(true)
    expect(result.current.livePlayMs).toBe(1000)

    const frozen = result.current.livePlayMs
    await act(async () => {
      vi.advanceTimersByTime(5000)
      await Promise.resolve()
    })
    expect(result.current.livePlayMs).toBe(frozen)
  })

  it('leaves the stall as soon as the phone reports a new play time', async () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return vi.fn()
    })
    mockReadMedia.mockResolvedValueOnce({
      timestamp: '2025-01-01T00:00:00Z',
      payload: {
        type: 1,
        media: { MediaPlayStatus: 1, MediaSongDuration: 600000, MediaSongPlayTime: 1000 }
      }
    })
    const { result } = renderHook(() => useMediaState(true))
    await act(async () => {
      await Promise.resolve()
    })
    await act(async () => {
      vi.advanceTimersByTime(3500)
      await Promise.resolve()
    })
    expect(result.current.stalled).toBe(true)

    const media = { MediaPlayStatus: 1, MediaSongDuration: 600000, MediaSongPlayTime: 2000 }
    ;(payloadFromLiveEvent as Mock).mockReturnValue({ type: 1, media })
    ;(mergePayload as Mock).mockReturnValue({ type: 1, media })
    act(() => {
      handler({}, { type: 'media', payload: { payload: { media } } })
    })
    expect(result.current.stalled).toBe(false)
    expect(result.current.livePlayMs).toBe(2000)
  })
})
