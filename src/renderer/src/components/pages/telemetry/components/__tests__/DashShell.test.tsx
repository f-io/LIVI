import { render, screen, waitFor } from '@testing-library/react'
import { DashShell } from '../DashShell'

type ResizeObserverCallback = (
  entries: Array<{ contentRect: { width: number; height: number } }>
) => void

describe('DashShell', () => {
  let resizeObserverCallback: ResizeObserverCallback | null = null
  const observeMock = jest.fn()
  const disconnectMock = jest.fn()

  beforeEach(() => {
    jest.clearAllMocks()
    resizeObserverCallback = null
    ;(global as any).ResizeObserver = class {
      constructor(cb: ResizeObserverCallback) {
        resizeObserverCallback = cb
      }

      observe = observeMock
      disconnect = disconnectMock
    }
  })

  test('renders children', () => {
    render(
      <DashShell>
        <div>Telemetry content</div>
      </DashShell>
    )

    expect(screen.getByText('Telemetry content')).toBeInTheDocument()
  })

  test('applies className to root element', () => {
    const { container } = render(
      <DashShell className="dash-shell-test">
        <div>Telemetry content</div>
      </DashShell>
    )

    expect(container.firstChild).toHaveClass('dash-shell-test')
  })

  test('starts with default scale 1 before resize information is available', () => {
    const { container } = render(
      <DashShell>
        <div>Telemetry content</div>
      </DashShell>
    )

    expect(container.firstChild).toHaveStyle('--dash-scale: 1')
  })

  test('observes the root element with ResizeObserver', () => {
    render(
      <DashShell>
        <div>Telemetry content</div>
      </DashShell>
    )

    expect(observeMock).toHaveBeenCalledTimes(1)
  })

  test('updates scale using default design size', async () => {
    const { container } = render(
      <DashShell>
        <div>Telemetry content</div>
      </DashShell>
    )

    resizeObserverCallback?.([{ contentRect: { width: 640, height: 360 } }])

    await waitFor(() => {
      expect(container.firstChild).toHaveStyle('--dash-scale: 0.5')
    })
  })

  test('updates scale using custom design size', async () => {
    const { container } = render(
      <DashShell designWidth={1000} designHeight={500}>
        <div>Telemetry content</div>
      </DashShell>
    )

    resizeObserverCallback?.([{ contentRect: { width: 500, height: 400 } }])

    await waitFor(() => {
      expect(container.firstChild).toHaveStyle('--dash-scale: 0.5')
    })
  })

  test('uses the smaller width/height ratio for scale', async () => {
    const { container } = render(
      <DashShell designWidth={1000} designHeight={500}>
        <div>Telemetry content</div>
      </DashShell>
    )

    resizeObserverCallback?.([{ contentRect: { width: 900, height: 200 } }])

    await waitFor(() => {
      expect(container.firstChild).toHaveStyle('--dash-scale: 0.4')
    })
  })

  test('disconnects ResizeObserver on unmount', () => {
    const { unmount } = render(
      <DashShell>
        <div>Telemetry content</div>
      </DashShell>
    )

    unmount()

    expect(disconnectMock).toHaveBeenCalledTimes(1)
  })

  test('ignores resize entries without contentRect', async () => {
    const { container } = render(
      <DashShell>
        <div>Telemetry content</div>
      </DashShell>
    )

    resizeObserverCallback?.([{} as never])

    await waitFor(() => {
      expect(container.firstChild).toHaveStyle('--dash-scale: 1')
    })
  })
})
