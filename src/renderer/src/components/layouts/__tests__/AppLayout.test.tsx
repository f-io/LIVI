import { act, fireEvent, render } from '@testing-library/react'
import { createRef } from 'react'
import { AppLayout } from '../AppLayout'

let mockPathname = '/'
let mockStreaming = false
let mockHand = 0

vi.mock('react-router', () => ({
  useLocation: () => ({ pathname: mockPathname })
}))

vi.mock('../../navigation', () => ({
  Nav: () => <div data-testid="nav">Nav</div>
}))

let mockTabCount = 4
vi.mock('../../navigation/useTabsConfig', () => ({
  useTabsConfig: () =>
    Array.from({ length: mockTabCount }, (_, i) => ({ path: `/${i}`, label: `t${i}`, icon: null }))
}))

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) => selector({ settings: { hand: mockHand } }),
  useStatusStore: (selector: (s: any) => unknown) => selector({ isStreaming: mockStreaming })
}))

vi.mock('../../../hooks/useBlinkingTime', () => ({
  useBlinkingTime: () => '12:34'
}))

let mockNetwork: { type: string; online: boolean } = { type: 'wifi', online: true }
vi.mock('../../../hooks/useNetworkStatus', () => ({
  useNetworkStatus: () => mockNetwork
}))

vi.mock('@mui/material/styles', async () => {
  const actual = await vi.importActual('@mui/material/styles')
  return {
    ...actual,
    useTheme: () => ({
      palette: { background: { paper: '#111' } }
    })
  }
})

describe('AppLayout', () => {
  beforeEach(async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    mockPathname = '/'
    mockStreaming = false
    mockHand = 0
    mockTabCount = 4
    mockNetwork = { type: 'wifi', online: true }
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 800 })
    ;(window as any).app = { notifyUserActivity: vi.fn() }
  })

  afterEach(async () => {
    vi.useRealTimers()
  })

  test('hides nav on home when streaming', async () => {
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

  test('auto-hides nav after inactivity on maps', async () => {
    mockPathname = '/telemetry'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')
    act(() => {
      vi.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')
  })

  test('forwards pointer activity to app notifier', async () => {
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

  test('shows nav again and re-arms hide timer on mousemove in maps mode', async () => {
    mockPathname = '/telemetry'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    act(() => {
      vi.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')

    fireEvent.mouseMove(document)

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')

    act(() => {
      vi.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')
  })

  test('shows nav again when focus moves into nav area on the telemetry page', async () => {
    mockPathname = '/telemetry'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const { container, getByTestId } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )

    act(() => {
      vi.advanceTimersByTime(3000)
    })
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('1')

    const navChild = getByTestId('nav')
    ;(navChild as HTMLElement).setAttribute('tabindex', '-1')
    ;(navChild as HTMLElement).focus()
    fireEvent.focusIn(navChild)

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')
  })

  test('clears auto-hide timer and keeps nav visible when leaving auto-hide pages', async () => {
    mockPathname = '/telemetry'
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
      vi.advanceTimersByTime(3000)
    })

    expect(container.querySelector('#content-root')?.getAttribute('data-nav-hidden')).toBe('0')
  })

  test('uses compact icons and a smaller clock at extra-small heights', async () => {
    Object.defineProperty(window, 'innerHeight', { configurable: true, value: 300 })
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect((container.querySelector('#nav-root') as HTMLElement).style.width).toBe('56px')
  })

  test('overlays the nav on auto-hide pages so the content keeps full width', async () => {
    mockPathname = '/telemetry'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    const nav = container.querySelector('#nav-root') as HTMLElement
    expect(nav.style.position).toBe('absolute')
    expect(nav.style.left).toBe('0px')
    expect(nav.style.right).toBe('')
  })

  test('anchors the overlay nav on the right for right-hand drive', async () => {
    mockPathname = '/telemetry'
    mockHand = 1
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    const nav = container.querySelector('#nav-root') as HTMLElement
    expect(nav.style.position).toBe('absolute')
    expect(nav.style.right).toBe('0px')
    expect(nav.style.left).toBe('')
  })

  test('keeps the nav in the layout flow on regular pages', async () => {
    mockPathname = '/media'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    const nav = container.querySelector('#nav-root') as HTMLElement
    expect(nav.style.position).toBe('relative')
  })

  test('mirrors the layout for a right-hand-drive steering position', async () => {
    mockHand = 1
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect((container.querySelector('#main') as HTMLElement).style.flexDirection).toBe(
      'row-reverse'
    )
    const nav = container.querySelector('#nav-root') as HTMLElement
    expect(nav.style.borderLeft).toContain('1px solid')
    expect(nav.style.borderRight).toBe('')
  })

  test('offsets a hidden nav to the right for right-hand-drive while streaming home', async () => {
    mockHand = 1
    mockStreaming = true
    mockPathname = '/'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect((container.querySelector('#nav-root') as HTMLElement).style.transform).toBe(
      'translateX(10px)'
    )
  })

  test('defaults the steering hand to left when the setting is absent', async () => {
    mockHand = undefined as unknown as number
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect((container.querySelector('#main') as HTMLElement).style.flexDirection).toBe('row')
  })

  test('shows the offline icon when the network is down and not on wifi', async () => {
    mockNetwork = { type: 'ethernet', online: false }
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect(container.querySelector('[data-testid="WifiOffIcon"]')).toBeTruthy()
  })

  test('shows no network icon when online but not on wifi', async () => {
    mockNetwork = { type: 'ethernet', online: true }
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect(container.querySelector('[data-testid="WifiIcon"]')).toBeFalsy()
    expect(container.querySelector('[data-testid="WifiOffIcon"]')).toBeFalsy()
  })

  test('marks the content region as nav-free when there is a single tab', async () => {
    mockTabCount = 1
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()
    const { container } = render(
      <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
        <div>Content</div>
      </AppLayout>
    )
    expect(container.querySelector('#content-root')?.getAttribute('data-nav-present')).toBe('0')
    expect(container.querySelector('#nav-root')).toBeNull()
  })

  test('removes wake listeners on unmount for auto-hide pages', async () => {
    mockPathname = '/telemetry'
    const navRef = createRef<HTMLDivElement>()
    const mainRef = createRef<HTMLDivElement>()

    const windowRemoveSpy = vi.spyOn(window, 'removeEventListener')
    const documentRemoveSpy = vi.spyOn(document, 'removeEventListener')

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
