import { render, screen } from '@testing-library/react'
import { DashPlaceholder } from '../DashPlaceholder'

describe('DashPlaceholder', () => {
  afterEach(() => {
    document.getElementById('content-root')?.remove()
    jest.restoreAllMocks()
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
    const disconnect = jest.fn()

    ;(global as any).MutationObserver = class {
      constructor(cb: MutationCallback) {
        observerCallback = cb
      }

      observe = jest.fn()
      disconnect = disconnect
    }

    render(<DashPlaceholder title="Telemetry Placeholder" />)

    expect(screen.getByText('Telemetry Placeholder')).toBeInTheDocument()

    el.setAttribute('data-nav-hidden', '1')
    observerCallback?.([], {} as MutationObserver)

    el.setAttribute('data-nav-hidden', '0')
    observerCallback?.([], {} as MutationObserver)
  })

  test('disconnects MutationObserver on unmount', () => {
    const el = document.createElement('div')
    el.id = 'content-root'
    el.setAttribute('data-nav-hidden', '1')
    document.body.appendChild(el)

    const disconnect = jest.fn()

    ;(global as any).MutationObserver = class {
      constructor(_: MutationCallback) {}

      observe = jest.fn()
      disconnect = disconnect
    }

    const { unmount } = render(<DashPlaceholder title="Telemetry Placeholder" />)

    unmount()

    expect(disconnect).toHaveBeenCalledTimes(1)
  })
})
