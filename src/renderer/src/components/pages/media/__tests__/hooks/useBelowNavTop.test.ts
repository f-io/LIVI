import { act, renderHook } from '@testing-library/react'
import { useBelowNavTop } from '../../hooks'

describe('useBelowNavTop', () => {
  let mockDisconnect: jest.Mock
  let mockObserve: jest.Mock
  let resizeObserverCallback: (() => void) | undefined
  let originalResizeObserver: typeof ResizeObserver
  let originalAddEventListener: typeof window.addEventListener
  let originalRemoveEventListener: typeof window.removeEventListener

  beforeEach(() => {
    document.body.innerHTML = ''

    mockDisconnect = jest.fn()
    mockObserve = jest.fn()
    resizeObserverCallback = undefined

    originalResizeObserver = global.ResizeObserver
    originalAddEventListener = window.addEventListener
    originalRemoveEventListener = window.removeEventListener

    global.ResizeObserver = jest.fn().mockImplementation((cb: () => void) => {
      resizeObserverCallback = cb
      return {
        observe: mockObserve,
        disconnect: mockDisconnect
      }
    }) as unknown as typeof ResizeObserver

    window.addEventListener = jest.fn()
    window.removeEventListener = jest.fn()
  })

  afterEach(() => {
    document.body.innerHTML = ''
    global.ResizeObserver = originalResizeObserver
    window.addEventListener = originalAddEventListener
    window.removeEventListener = originalRemoveEventListener
    jest.restoreAllMocks()
  })

  const appendNav = (bottom = 42) => {
    const mockNav = document.createElement('div')
    mockNav.className = 'MuiTabs-root'
    mockNav.getBoundingClientRect = jest.fn(() => ({ bottom })) as never
    document.body.appendChild(mockNav)
    return mockNav
  }

  it('reads initial nav bottom on mount', () => {
    appendNav(42)

    const { result } = renderHook(() => useBelowNavTop())

    expect(result.current).toBe(42)
    expect(window.addEventListener).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(mockObserve).toHaveBeenCalledTimes(1)
  })

  it('updates top when nav position changes via ResizeObserver', () => {
    const mockNav = appendNav(42)
    const { result } = renderHook(() => useBelowNavTop())

    ;(mockNav.getBoundingClientRect as jest.Mock).mockReturnValue({ bottom: 99 })

    act(() => {
      resizeObserverCallback?.()
    })

    expect(result.current).toBe(99)
  })

  it('updates top when window resize listener runs', () => {
    const mockNav = appendNav(42)
    const { result } = renderHook(() => useBelowNavTop())

    ;(mockNav.getBoundingClientRect as jest.Mock).mockReturnValue({ bottom: 77 })

    const resizeHandler = (window.addEventListener as jest.Mock).mock.calls.find(
      ([event]) => event === 'resize'
    )?.[1] as (() => void) | undefined

    act(() => {
      resizeHandler?.()
    })

    expect(result.current).toBe(77)
  })

  it('clamps negative nav bottom to 0', () => {
    appendNav(-12)

    const { result } = renderHook(() => useBelowNavTop())

    expect(result.current).toBe(0)
  })

  it('returns 0 and does not observe when no nav element exists', () => {
    const { result } = renderHook(() => useBelowNavTop())

    expect(result.current).toBe(0)
    expect(mockObserve).not.toHaveBeenCalled()
  })

  it('cleans up event listeners and ResizeObserver on unmount', () => {
    appendNav(42)

    const { unmount } = renderHook(() => useBelowNavTop())
    unmount()

    expect(window.removeEventListener).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(mockDisconnect).toHaveBeenCalledTimes(1)
  })

  it('cleans up safely without ResizeObserver instance when no nav exists', () => {
    const { unmount } = renderHook(() => useBelowNavTop())
    unmount()

    expect(window.removeEventListener).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(mockDisconnect).not.toHaveBeenCalled()
  })
})
