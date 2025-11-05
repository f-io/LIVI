import { renderHook, act } from '@testing-library/react'
import { useBelowNavTop } from '../../hooks'

describe('useBelowNavTop', () => {
  let mockDisconnect: jest.Mock
  let mockObserve: jest.Mock
  let originalResizeObserver: typeof ResizeObserver
  let originalAddEventListener: typeof window.addEventListener
  let originalRemoveEventListener: typeof window.removeEventListener

  beforeEach(() => {
    mockDisconnect = jest.fn()
    mockObserve = jest.fn()

    // Save originals
    originalResizeObserver = global.ResizeObserver
    originalAddEventListener = window.addEventListener
    originalRemoveEventListener = window.removeEventListener

    // Mock ResizeObserver
    global.ResizeObserver = jest.fn().mockImplementation(() => ({
      observe: mockObserve,
      disconnect: mockDisconnect
    })) as unknown as typeof ResizeObserver

    // Mock add/removeEventListener
    window.addEventListener = jest.fn()
    window.removeEventListener = jest.fn()

    // Mock DOM element
    const mockNav = document.createElement('div')
    mockNav.className = 'MuiTabs-root'
    mockNav.getBoundingClientRect = jest.fn(() => ({ bottom: 42 })) as never
    document.body.appendChild(mockNav)
  })

  afterEach(() => {
    document.body.innerHTML = ''
    global.ResizeObserver = originalResizeObserver
    window.addEventListener = originalAddEventListener
    window.removeEventListener = originalRemoveEventListener
    jest.restoreAllMocks()
  })

  it('returns initial top = 0 before effect runs', () => {
    const { result } = renderHook(() => useBelowNavTop())
    expect(result.current).toBe(42)
  })

  it('updates top when nav position changes via ResizeObserver', () => {
    const mockNav = document.querySelector('.MuiTabs-root') as HTMLElement
    ;(mockNav.getBoundingClientRect as jest.Mock).mockReturnValue({ bottom: 99 })

    const { result } = renderHook(() => useBelowNavTop())

    act(() => {
      const callback = (global.ResizeObserver as jest.Mock).mock.calls[0][0]
      callback()
    })

    expect(result.current).toBe(99)
  })

  it('returns 0 if no .MuiTabs-root element is found', () => {
    document.body.innerHTML = ''
    const { result } = renderHook(() => useBelowNavTop())
    expect(result.current).toBe(0)
  })

  it('cleans up event listeners and ResizeObserver on unmount', () => {
    const { unmount } = renderHook(() => useBelowNavTop())
    unmount()

    expect(window.removeEventListener).toHaveBeenCalledWith('resize', expect.any(Function))
    expect(mockDisconnect).toHaveBeenCalled()
  })
})
