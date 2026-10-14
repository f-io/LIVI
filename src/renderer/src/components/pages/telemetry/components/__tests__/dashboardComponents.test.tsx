import { render, screen } from '@testing-library/react'
import { DashboardPlaceholder } from '../DashboardPlaceholder'
import { DashboardShell } from '../DashboardShell'

describe('DashboardShell', () => {
  test('renders children and listens for window resize', () => {
    const addSpy = vi.spyOn(window, 'addEventListener')
    const removeSpy = vi.spyOn(window, 'removeEventListener')

    const { unmount } = render(
      <div style={{ width: 400, height: 300 }}>
        <DashboardShell>
          <div>dashboard-child</div>
        </DashboardShell>
      </div>
    )

    expect(screen.getByText('dashboard-child')).toBeInTheDocument()
    expect(addSpy).toHaveBeenCalledWith('resize', expect.any(Function))

    unmount()
    expect(removeSpy).toHaveBeenCalledWith('resize', expect.any(Function))

    addSpy.mockRestore()
    removeSpy.mockRestore()
  })
})

describe('DashboardPlaceholder', () => {
  test('renders title and subscribes to content-root attribute changes', () => {
    const contentRoot = document.createElement('div')
    contentRoot.id = 'content-root'
    document.body.appendChild(contentRoot)

    const observe = vi.fn()
    const disconnect = vi.fn()
    ;(global as any).MutationObserver = vi.fn(function () {
      return { observe, disconnect }
    })

    const { unmount } = render(<DashboardPlaceholder title="Telemetry" />)

    expect(screen.getByText('Telemetry')).toBeInTheDocument()
    expect(observe).toHaveBeenCalledWith(contentRoot, {
      attributes: true,
      attributeFilter: ['data-nav-hidden']
    })

    unmount()
    expect(disconnect).toHaveBeenCalledTimes(1)
    contentRoot.remove()
  })
})
