import { renderHook, act } from '@testing-library/react'
import { useMediaState } from '../../hooks'
import { clamp, mergePayload, payloadFromLiveEvent } from '../../utils'

jest.mock('../../utils/clamp', () => ({
  clamp: jest.fn((n: number, min: number, max: number) => Math.max(min, Math.min(max, n)))
}))
jest.mock('../../utils/mergePayload', () => ({
  mergePayload: jest.fn()
}))
jest.mock('../../utils/payloadFromLiveEvent', () => ({
  payloadFromLiveEvent: jest.fn()
}))

const mockReadMedia = jest.fn()
const mockOnEvent = jest.fn()
const mockRemoveListener = jest.fn()

beforeEach(() => {
  jest.useFakeTimers()

  jest.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
    setTimeout(() => cb(performance.now()), 16)
    return 1
  })
  jest.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {})
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-expect-error
  ;(window as never).carplay = {
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
  ;(clamp as jest.Mock).mockClear()
  ;(mergePayload as jest.Mock).mockClear()
  ;(payloadFromLiveEvent as jest.Mock).mockClear()
})

afterEach(() => {
  jest.useRealTimers()
  jest.restoreAllMocks()
})

describe('useMediaState', () => {
  it('returns initial state', () => {
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

  it('subscribes to carplay events and unsubscribes on unmount', () => {
    const unsubscribe = jest.fn()
    mockOnEvent.mockReturnValueOnce(unsubscribe)

    const { unmount } = renderHook(() => useMediaState(false))

    expect(mockOnEvent).toHaveBeenCalled()
    unmount()
    expect(unsubscribe).toHaveBeenCalled()
  })

  it('falls back to electron.removeListener when unsubscribe not provided', () => {
    mockOnEvent.mockReturnValueOnce(undefined)

    const { unmount } = renderHook(() => useMediaState(false))
    unmount()

    expect(mockRemoveListener).toHaveBeenCalledWith('carplay-event', expect.any(Function))
  })

  it('handles unplugged event correctly', () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return jest.fn()
    })

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'unplugged' })
    })

    expect(result.current.snap).toBeNull()
    expect(result.current.livePlayMs).toBe(0)
  })

  it('handles valid live event and updates state', () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return jest.fn()
    })

    const inc = {
      type: 1,
      media: { MediaSongPlayTime: 50 }
    }
    const merged = {
      type: 1,
      media: { MediaSongPlayTime: 50 }
    }

    ;(payloadFromLiveEvent as jest.Mock).mockReturnValue(inc)
    ;(mergePayload as jest.Mock).mockReturnValue(merged)

    const { result } = renderHook(() => useMediaState(false))

    act(() => {
      handler({}, { type: 'media', payload: { payload: inc } })
    })

    expect(payloadFromLiveEvent).toHaveBeenCalled()
    expect(mergePayload).toHaveBeenCalled()
    expect(result.current.livePlayMs).toBe(50)
    expect(result.current.snap?.payload.media?.MediaSongPlayTime).toBe(50)
  })

  it('does not call mergePayload when payloadFromLiveEvent returns null', () => {
    let handler: (ev: unknown, ...args: unknown[]) => void = () => {}
    mockOnEvent.mockImplementationOnce((cb) => {
      handler = cb
      return jest.fn()
    })
    ;(payloadFromLiveEvent as jest.Mock).mockReturnValue(null)

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
        jest.advanceTimersByTime(50)
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
        jest.advanceTimersByTime(50)
        await Promise.resolve()
      })
    }

    expect(clamp).not.toHaveBeenCalled()
  })
})
