import { act, fireEvent, render } from '@testing-library/react'
import { createRef } from 'react'
import { AppLayout } from '../AppLayout'

let mockPathname = '/'
let mockStreaming = false
let mockHand = 0

jest.mock('react-router', () => ({
  useLocation: () => ({ pathname: mockPathname })
}))

jest.mock('../../navigation', () => ({
  Nav: () => <div data-testid="nav">Nav</div>
}))

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) => selector({ settings: { hand: mockHand } }),
  useStatusStore: (selector: (s: any) => unknown) => selector({ isStreaming: mockStreaming })
}))

jest.mock('../../../hooks/useBlinkingTime', () => ({
  useBlinkingTime: () => '12:34'
}))

jest.mock('../../../hooks/useNetworkStatus', () => ({
  useNetworkStatus: () => ({ type: 'wifi', online: true })
}))

jest.mock('@mui/material/styles', () => {
  const actual = jest.requireActual('@mui/material/styles')
  return {
    ...actual,
    useTheme: () => ({
      palette: { background: { paper: '#111' } }
    })
  }
})

describe('AppLayout', () => {
  beforeEach(() => {
    jest.useFakeTimers()
    mockPathname = '/'
    mockStreaming = false
    mockHand = 0
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 800 })
    ;(window as any).app = { notifyUserActivity: jest.fn() }
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  test('hides nav on home when streaming', () => {
    mockStreaming = true
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')
  })

  test('auto-hides nav after inactivity on maps', () => {
    mockPathname = '/cluster'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')
    act(() => {
      jest.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')
  })

  test('forwards pointer activity to app notifier', () => {
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    fireEvent.pointerDown(container.querySelector('#main') as HTMLElement)
    expect((window as any).app.notifyUserActivity).toHaveBeenCalled()
  })

  test('shows nav again and re-arms hide timer on mousemove in maps mode', () => {
    mockPathname = '/cluster'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    act(() => {
      jest.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')

    fireEvent.mouseMove(document)

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')

    act(() => {
      jest.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')
  })

  test('shows nav again when focus moves into nav area on cluster page', () => {
    mockPathname = '/cluster'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const { container, getByTestId } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    act(() => {
      jest.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')

    const navChild = getByTestId('nav')
    ;(navChild as HTMLElement).setAttribute('tabindex', '-1')
    ;(navChild as HTMLElement).focus()
    fireEvent.focusIn(navChild)

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')
  })

  test('clears auto-hide timer and keeps nav visible when leaving auto-hide pages', () => {
    mockPathname = '/cluster'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const { container, rerender } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')

    mockPathname = '/'
    rerender(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    act(() => {
      jest.advanceTimersByTime(3000)
    })

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')
  })

  test('removes wake listeners on unmount for auto-hide pages', () => {
    mockPathname = '/cluster'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const windowRemoveSpy = jest.spyOn(window, 'removeEventListener')
    const documentRemoveSpy = jest.spyOn(document, 'removeEventListener')

    const { unmount } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    unmount()

    expect(windowRemoveSpy).toHaveBeenCalledWith('keydown', expect.any(Function))
    expect(documentRemoveSpy).toHaveBeenCalledWith('mousemove', expect.any(Function))
    expect(documentRemoveSpy).toHaveBeenCalledWith('wheel', expect.any(Function))
    expect(documentRemoveSpy).toHaveBeenCalledWith('focusin', expect.any(Function))
  })
})
