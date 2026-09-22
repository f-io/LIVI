import { act, render, screen } from '@testing-library/react'
import { DashPlaceholder } from '../DashPlaceholder'

describe('DashPlaceholder', () => {
  afterEach(() => {
    document.getElementById('content-root')?.remove()
    vi.restoreAllMocks()
  })

  test('renders title even when content-root is missing', () => {
    render(<DashPlaceholder title="Telemetry Placeholder" />)

    expect(screen.getByText('Telemetry Placeholder')).toBeInTheDocument()
  })

  test('reacts to nav hidden attribute changes', () => {
    const el = document.createElement('div')
    el.id = 'content-root'
    el.setAttribute('data-nav-hidden', '0')
    document.body.appendChild(el)

    let observerCallback: MutationCallback | undefined
    const disconnect = vi.fn()

    ;(global as any).MutationObserver = class {
      constructor(cb: MutationCallback) {
        observerCallback = cb
      }

      observe = vi.fn()
      disconnect = disconnect
    }

    render(<DashPlaceholder title="Telemetry Placeholder" />)

    const fill = screen.getByText('Telemetry Placeholder').parentElement as HTMLElement
    expect(fill).toHaveStyle({ position: 'absolute' })

    el.setAttribute('data-nav-hidden', '1')
    act(() => observerCallback?.([], {} as MutationObserver))
    expect(fill).toHaveStyle({ position: 'fixed' })

    el.setAttribute('data-nav-hidden', '0')
    act(() => observerCallback?.([], {} as MutationObserver))
    expect(fill).toHaveStyle({ position: 'absolute' })
  })

  test('disconnects MutationObserver on unmount', () => {
    const el = document.createElement('div')
    el.id = 'content-root'
    el.setAttribute('data-nav-hidden', '1')
    document.body.appendChild(el)

    const disconnect = vi.fn()

    ;(global as any).MutationObserver = class {
      constructor(_: MutationCallback) {}

      observe = vi.fn()
      disconnect = disconnect
    }

    const { unmount } = render(<DashPlaceholder title="Telemetry Placeholder" />)

    unmount()

    expect(disconnect).toHaveBeenCalledTimes(1)
  })
})
