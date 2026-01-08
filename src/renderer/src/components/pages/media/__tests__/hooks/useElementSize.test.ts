import { renderHook } from '@testing-library/react'
import { useElementSize } from '../../hooks'

describe('useElementSize', () => {
  let mockObserve: jest.Mock
  let mockDisconnect: jest.Mock

  beforeEach(() => {
    mockObserve = jest.fn()
    mockDisconnect = jest.fn()

    // Mock ResizeObserver
    global.ResizeObserver = jest.fn(() => ({
      observe: mockObserve,
      disconnect: mockDisconnect
    })) as unknown as typeof ResizeObserver

    // Mock requestAnimationFrame / cancelAnimationFrame
    jest.spyOn(window, 'requestAnimationFrame').mockImplementation((cb) => {
      cb(0)
      return 1
    })
    jest.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => {})
  })

  afterEach(() => {
    jest.restoreAllMocks()
  })

  it('initializes with window size', () => {
    const { result } = renderHook(() => useElementSize<HTMLDivElement>())

    const [, size] = result.current
    expect(size).toEqual({
      w: window.innerWidth,
      h: window.innerHeight
    })
  })

  it('observes element when ref is set before mount', () => {
    const div = document.createElement('div')

    // We pass the ref in advance with the element already set
    renderHook(() => {
      const [ref, size] = useElementSize<HTMLDivElement>()
      ref.current = div
      return [ref, size] as const
    })

    expect(mockObserve).toHaveBeenCalledWith(div)
  })

  it('cleans up observer and animation frame on unmount', () => {
    const div = document.createElement('div')

    const { unmount } = renderHook(() => {
      const [ref] = useElementSize<HTMLDivElement>()
      ref.current = div
      return ref
    })

    unmount()

    expect(mockDisconnect).toHaveBeenCalled()
    expect(window.cancelAnimationFrame).not.toThrow()
  })
})
