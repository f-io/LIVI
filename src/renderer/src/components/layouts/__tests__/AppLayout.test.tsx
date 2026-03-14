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
    mockPathname = '/maps'
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
})
