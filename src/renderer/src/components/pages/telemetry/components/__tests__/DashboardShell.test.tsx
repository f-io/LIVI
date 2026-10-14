import { act, fireEvent, render } from '@testing-library/react'
import { DashboardShell } from '../DashboardShell'

function setWindowSize(w: number, h: number): void {
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: w })
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: h })
}

describe('DashboardShell', () => {
  beforeEach(async () => {
    vi.clearAllMocks()
    setWindowSize(1024, 768)
  })

  test('derives the initial scale from the window before resize information is available', async () => {
    const { container } = render(
      <DashboardShell>
        <div>Telemetry content</div>
      </DashboardShell>
    )

    expect(container.firstChild).toHaveStyle('--dashboard-scale: 0.8')
  })

  test('falls back to scale 1 while the window reports no size', async () => {
    setWindowSize(0, 0)
    const { container } = render(
      <DashboardShell>
        <div>Telemetry content</div>
      </DashboardShell>
    )

    expect(container.firstChild).toHaveStyle('--dashboard-scale: 1')
  })

  test('applies a settled window resize using the default design size', () => {
    vi.useFakeTimers()
    try {
      const { container } = render(
        <DashboardShell>
          <div>Telemetry content</div>
        </DashboardShell>
      )

      setWindowSize(640, 360)
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(150)
      })

      expect(container.firstChild).toHaveStyle('--dashboard-scale: 0.5')
    } finally {
      vi.useRealTimers()
    }
  })

  test('applies a settled window resize using a custom design size', () => {
    vi.useFakeTimers()
    try {
      const { container } = render(
        <DashboardShell designWidth={1000} designHeight={500}>
          <div>Telemetry content</div>
        </DashboardShell>
      )

      setWindowSize(500, 400)
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(150)
      })

      expect(container.firstChild).toHaveStyle('--dashboard-scale: 0.5')
    } finally {
      vi.useRealTimers()
    }
  })

  test('uses the smaller width/height ratio for scale', () => {
    vi.useFakeTimers()
    try {
      const { container } = render(
        <DashboardShell designWidth={1000} designHeight={500}>
          <div>Telemetry content</div>
        </DashboardShell>
      )

      setWindowSize(900, 200)
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(150)
      })

      expect(container.firstChild).toHaveStyle('--dashboard-scale: 0.4')
    } finally {
      vi.useRealTimers()
    }
  })

  test('collapses a transient resize blip into a single settled update', () => {
    vi.useFakeTimers()
    try {
      const { container } = render(
        <DashboardShell>
          <div>Telemetry content</div>
        </DashboardShell>
      )

      setWindowSize(1022, 768)
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(50)
      })
      setWindowSize(1024, 768)
      fireEvent(window, new Event('resize'))
      act(() => {
        vi.advanceTimersByTime(150)
      })

      expect(container.firstChild).toHaveStyle('--dashboard-scale: 0.8')
    } finally {
      vi.useRealTimers()
    }
  })

  test('cancels a pending resize and stops listening on unmount', () => {
    vi.useFakeTimers()
    try {
      const { container, unmount } = render(
        <DashboardShell>
          <div>Telemetry content</div>
        </DashboardShell>
      )

      setWindowSize(640, 360)
      fireEvent(window, new Event('resize'))
      unmount()
      act(() => {
        vi.advanceTimersByTime(300)
      })

      expect(container.firstChild).toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })
})
