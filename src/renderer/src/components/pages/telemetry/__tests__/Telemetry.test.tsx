import { AppContext } from '@renderer/context'
import { fireEvent, render, screen } from '@testing-library/react'
import React from 'react'
import { Telemetry } from '../Telemetry'

const useLiviStoreMock = vi.fn()
const useNavbarHiddenMock = vi.fn()
const useKeyboardNavigationMock = vi.fn()
const normalizeDashboardComponentsMock = vi.fn()

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (state: { settings: unknown }) => unknown) => useLiviStoreMock(selector)
}))

vi.mock('@renderer/hooks/useNavbarHidden', () => ({
  useNavbarHidden: () => useNavbarHiddenMock()
}))

vi.mock('../hooks/useKeyboardNavigation', () => ({
  useKeyboardNavigation: (args: unknown) => useKeyboardNavigationMock(args)
}))

vi.mock('@renderer/components/pages/telemetry/utils', () => ({
  normalizeDashboardComponents: (...args: unknown[]) => normalizeDashboardComponentsMock(...args)
}))

vi.mock('@renderer/components/pages/telemetry/config', () => ({
  DashboardConfig: {
    dashboard1: React.createElement('div', { 'data-testid': 'dashboard-1' }, 'Dashboard 1'),
    dashboard2: React.createElement('div', { 'data-testid': 'dashboard-2' }, 'Dashboard 2')
  }
}))

vi.mock('@renderer/components/pages/telemetry/components/DashboardPlaceholder', () => ({
  DashboardPlaceholder: ({ title }: { title: string }) =>
    React.createElement('div', { 'data-testid': 'dashboard-placeholder' }, title)
}))

vi.mock('@renderer/components/pages/telemetry/components/pagination/pagination', () => ({
  DashboardsPagination: ({
    activeIndex,
    dotsLength,
    onSetIndex,
    isNavbarHidden
  }: {
    activeIndex: number
    dotsLength: number
    onSetIndex: (index: number) => void
    isNavbarHidden: boolean
  }) =>
    React.createElement(
      'button',
      {
        'data-testid': 'pagination',
        'data-active-index': activeIndex,
        'data-dots-length': dotsLength,
        'data-navbar-hidden': String(isNavbarHidden),
        onClick: () => onSetIndex(1)
      },
      'pagination'
    )
}))

vi.mock('@renderer/context', async () => {
  const React = await import('react')
  return {
    AppContext: React.createContext({})
  }
})

const renderWithContext = (ui: React.ReactElement, value: Record<string, unknown> = {}) => {
  return render(
    <AppContext.Provider
      value={
        {
          isTouchDevice: false,
          ...(value as object)
        } as any
      }
    >
      {ui}
    </AppContext.Provider>
  )
}

describe('Telemetry', () => {
  beforeEach(async () => {
    vi.clearAllMocks()

    useLiviStoreMock.mockImplementation((selector: (state: { settings: unknown }) => unknown) =>
      selector({
        settings: {
          telemetryDashboards: [{ id: 'dashboard1', enabled: true, pos: 1 }]
        }
      })
    )

    useNavbarHiddenMock.mockReturnValue({ isNavbarHidden: false })

    normalizeDashboardComponentsMock.mockReturnValue({
      dashboards: [{ id: 'dashboard1', pos: 1 }]
    })

    useKeyboardNavigationMock.mockReturnValue({
      prev: vi.fn(),
      next: vi.fn(),
      canPrev: false,
      canNext: false,
      onPointerDown: vi.fn(),
      onPointerUp: vi.fn()
    })
  })

  test('renders fallback when no dashboards are enabled', async () => {
    normalizeDashboardComponentsMock.mockReturnValue({ dashboards: [] })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dashboard-placeholder')).toHaveTextContent('No dashboards enabled')
  })

  test('renders configured dashboard for current index', async () => {
    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dashboard-1')).toBeInTheDocument()
  })

  test('renders unknown fallback when dashboard id is not in config', async () => {
    normalizeDashboardComponentsMock.mockReturnValue({
      dashboards: [{ id: 'unknownDash', pos: 1 }]
    })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dashboard-placeholder')).toHaveTextContent('Unknown dashboard')
  })

  test('renders pagination only when more than one dashboard exists', async () => {
    normalizeDashboardComponentsMock.mockReturnValue({
      dashboards: [
        { id: 'dashboard1', pos: 1 },
        { id: 'dashboard2', pos: 2 }
      ]
    })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('pagination')).toBeInTheDocument()
  })

  test('does not render pagination when only one dashboard exists', async () => {
    renderWithContext(<Telemetry />)

    expect(screen.queryByTestId('pagination')).toBeNull()
  })

  test('switches dashboard when pagination changes index', async () => {
    normalizeDashboardComponentsMock.mockReturnValue({
      dashboards: [
        { id: 'dashboard1', pos: 1 },
        { id: 'dashboard2', pos: 2 }
      ]
    })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dashboard-1')).toBeInTheDocument()

    fireEvent.click(screen.getByTestId('pagination'))

    expect(screen.getByTestId('dashboard-2')).toBeInTheDocument()
  })

  test('passes telemetry pager into app context and cleans up on unmount', async () => {
    const prev = vi.fn()
    const next = vi.fn()
    const onSetAppContext = vi.fn()

    useKeyboardNavigationMock.mockReturnValue({
      prev,
      next,
      canPrev: true,
      canNext: true,
      onPointerDown: vi.fn(),
      onPointerUp: vi.fn()
    })

    const { unmount } = renderWithContext(<Telemetry />, { onSetAppContext })

    expect(onSetAppContext).toHaveBeenCalledWith({
      telemetryPager: { prev, next, canPrev: true, canNext: true }
    })

    unmount()

    expect(onSetAppContext).toHaveBeenLastCalledWith({
      telemetryPager: undefined
    })
  })

  test('does not try to register app context when onSetAppContext is missing', async () => {
    renderWithContext(<Telemetry />, {})

    expect(screen.getByTestId('dashboard-1')).toBeInTheDocument()
  })

  test('uses fixed positioning when navbar is hidden', async () => {
    useNavbarHiddenMock.mockReturnValue({ isNavbarHidden: true })

    const { container } = renderWithContext(<Telemetry />)

    expect(container.firstChild).toHaveStyle({ position: 'fixed' })
  })

  test('falls back when the active index points past the available dashboards', async () => {
    normalizeDashboardComponentsMock.mockReturnValue({
      dashboards: [
        { id: 'dashboard1', pos: 1 },
        { id: 'dashboard2', pos: 2 }
      ]
    })

    const { rerender } = renderWithContext(<Telemetry />)

    fireEvent.click(screen.getByTestId('pagination'))
    expect(screen.getByTestId('dashboard-2')).toBeInTheDocument()

    normalizeDashboardComponentsMock.mockReturnValue({
      dashboards: [{ id: 'dashboard1', pos: 1 }]
    })

    rerender(
      <AppContext.Provider value={{ isTouchDevice: false } as any}>
        <Telemetry />
      </AppContext.Provider>
    )

    expect(screen.getByTestId('dashboard-1')).toBeInTheDocument()
  })

  test('wires pointer handlers from keyboard navigation hook', async () => {
    const onPointerDown = vi.fn()
    const onPointerUp = vi.fn()

    useKeyboardNavigationMock.mockReturnValue({
      prev: vi.fn(),
      next: vi.fn(),
      canPrev: false,
      canNext: false,
      onPointerDown,
      onPointerUp
    })

    const { container } = renderWithContext(<Telemetry />)

    fireEvent.pointerDown(container.firstChild as Element)
    fireEvent.pointerUp(container.firstChild as Element)

    expect(onPointerDown).toHaveBeenCalled()
    expect(onPointerUp).toHaveBeenCalled()
  })
})
