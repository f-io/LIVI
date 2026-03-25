import { act, render, screen } from '@testing-library/react'
import { useElementSize } from '../useElementSize'

describe('useElementSize', () => {
  let resizeObserverCallback:
    | ((entries: Array<{ contentRect?: { width: number; height: number } }>) => void)
    | undefined

  let observeMock: jest.Mock
  let disconnectMock: jest.Mock
  let requestAnimationFrameMock: jest.Mock
  let cancelAnimationFrameMock: jest.Mock

  beforeEach(() => {
    jest.clearAllMocks()
    resizeObserverCallback = undefined

    observeMock = jest.fn()
    disconnectMock = jest.fn()

    Object.defineProperty(window, 'innerWidth', {
      configurable: true,
      value: 1280
    })

    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      value: 720
    })

    requestAnimationFrameMock = jest.fn((cb: FrameRequestCallback) => {
      cb(0)
      return 1
    })

    cancelAnimationFrameMock = jest.fn()
    ;(global as any).requestAnimationFrame = requestAnimationFrameMock
    ;(global as any).cancelAnimationFrame = cancelAnimationFrameMock
    ;(global as any).ResizeObserver = class {
      constructor(
        cb: (entries: Array<{ contentRect?: { width: number; height: number } }>) => void
      ) {
        resizeObserverCallback = cb
      }

      observe = observeMock
      disconnect = disconnectMock
    }
  })

  function TestComponent() {
    const [ref, size] = useElementSize<HTMLDivElement>()

    return (
      <div>
        <div ref={ref} data-testid="observed" />
        <div data-testid="size">
          {size.w}x{size.h}
        </div>
      </div>
    )
  }

  test('returns window size as initial fallback', () => {
    render(<TestComponent />)

    expect(screen.getByTestId('size')).toHaveTextContent('1280x720')
  })

  test('observes attached element and updates rounded size', () => {
    render(<TestComponent />)

    expect(observeMock).toHaveBeenCalledWith(screen.getByTestId('observed'))

    act(() => {
      resizeObserverCallback?.([{ contentRect: { width: 123.4, height: 456.6 } }])
    })

    expect(screen.getByTestId('size')).toHaveTextContent('123x457')
  })

  test('does not update size when rounded values stay the same', () => {
    render(<TestComponent />)

    act(() => {
      resizeObserverCallback?.([{ contentRect: { width: 200.2, height: 300.2 } }])
    })

    expect(screen.getByTestId('size')).toHaveTextContent('200x300')

    act(() => {
      resizeObserverCallback?.([{ contentRect: { width: 200.4, height: 300.4 } }])
    })

    expect(screen.getByTestId('size')).toHaveTextContent('200x300')
  })

  test('ignores resize entries without contentRect', () => {
    render(<TestComponent />)

    act(() => {
      resizeObserverCallback?.([{}])
    })

    expect(screen.getByTestId('size')).toHaveTextContent('1280x720')
  })

  test('disconnects observer on unmount', () => {
    const { unmount } = render(<TestComponent />)

    unmount()

    expect(disconnectMock).toHaveBeenCalledTimes(1)
  })

  test('cancels scheduled animation frame on unmount when one is pending', () => {
    requestAnimationFrameMock = jest.fn(() => 42)
    ;(global as any).requestAnimationFrame = requestAnimationFrameMock

    const { unmount } = render(<TestComponent />)

    act(() => {
      resizeObserverCallback?.([{ contentRect: { width: 500, height: 250 } }])
    })

    expect(requestAnimationFrameMock).toHaveBeenCalled()

    unmount()

    expect(cancelAnimationFrameMock).toHaveBeenCalledWith(42)
  })
})
