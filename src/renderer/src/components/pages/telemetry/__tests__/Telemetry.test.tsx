import React from 'react'
import { fireEvent, render, screen } from '@testing-library/react'
import { Telemetry } from '../Telemetry'
import { AppContext } from '@renderer/context'

const useLiviStoreMock = jest.fn()
const useNavbarHiddenMock = jest.fn()
const useKeyboardNavigationMock = jest.fn()
const normalizeDashComponentsMock = jest.fn()

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (state: { settings: unknown }) => unknown) => useLiviStoreMock(selector)
}))

jest.mock('@renderer/hooks/useNavbarHidden', () => ({
  useNavbarHidden: () => useNavbarHiddenMock()
}))

jest.mock('../hooks/useKeyboardNavigation', () => ({
  useKeyboardNavigation: (args: unknown) => useKeyboardNavigationMock(args)
}))

jest.mock('@renderer/components/pages/telemetry/utils', () => ({
  normalizeDashComponents: (...args: unknown[]) => normalizeDashComponentsMock(...args)
}))

jest.mock('@renderer/components/pages/telemetry/config', () => ({
  DashboardConfig: {
    dash1: React.createElement('div', { 'data-testid': 'dash-1' }, 'Dash 1'),
    dash2: React.createElement('div', { 'data-testid': 'dash-2' }, 'Dash 2')
  }
}))

jest.mock('@renderer/components/pages/telemetry/components/DashPlaceholder', () => ({
  DashPlaceholder: ({ title }: { title: string }) =>
    React.createElement('div', { 'data-testid': 'dash-placeholder' }, title)
}))

jest.mock('@renderer/components/pages/telemetry/components/pagination/pagination', () => ({
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

jest.mock('@renderer/context', () => {
  const React = require('react')
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
  beforeEach(() => {
    jest.clearAllMocks()

    useLiviStoreMock.mockImplementation((selector: (state: { settings: unknown }) => unknown) =>
      selector({
        settings: {
          telemetryDashboards: [{ id: 'dash1', enabled: true, pos: 1 }]
        }
      })
    )

    useNavbarHiddenMock.mockReturnValue({ isNavbarHidden: false })

    normalizeDashComponentsMock.mockReturnValue({
      dashboards: [{ id: 'dash1', pos: 1 }]
    })

    useKeyboardNavigationMock.mockReturnValue({
      prev: jest.fn(),
      next: jest.fn(),
      canPrev: false,
      canNext: false,
      onPointerDown: jest.fn(),
      onPointerUp: jest.fn()
    })
  })

  test('renders fallback when no dashboards are enabled', () => {
    normalizeDashComponentsMock.mockReturnValue({ dashboards: [] })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dash-placeholder')).toHaveTextContent('No dashboards enabled')
  })

  test('renders configured dashboard for current index', () => {
    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dash-1')).toBeInTheDocument()
  })

  test('renders unknown fallback when dashboard id is not in config', () => {
    normalizeDashComponentsMock.mockReturnValue({
      dashboards: [{ id: 'unknownDash', pos: 1 }]
    })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dash-placeholder')).toHaveTextContent('Unknown dash')
  })

  test('renders pagination only when more than one dashboard exists', () => {
    normalizeDashComponentsMock.mockReturnValue({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash2', pos: 2 }
      ]
    })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('pagination')).toBeInTheDocument()
  })

  test('does not render pagination when only one dashboard exists', () => {
    renderWithContext(<Telemetry />)

    expect(screen.queryByTestId('pagination')).toBeNull()
  })

  test('switches dashboard when pagination changes index', () => {
    normalizeDashComponentsMock.mockReturnValue({
      dashboards: [
        { id: 'dash1', pos: 1 },
        { id: 'dash2', pos: 2 }
      ]
    })

    renderWithContext(<Telemetry />)

    expect(screen.getByTestId('dash-1')).toBeInTheDocument()

    fireEvent.click(screen.getByTestId('pagination'))

    expect(screen.getByTestId('dash-2')).toBeInTheDocument()
  })

  test('passes telemetry pager into app context and cleans up on unmount', () => {
    const prev = jest.fn()
    const next = jest.fn()
    const onSetAppContext = jest.fn()

    useKeyboardNavigationMock.mockReturnValue({
      prev,
      next,
      canPrev: true,
      canNext: true,
      onPointerDown: jest.fn(),
      onPointerUp: jest.fn()
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

  test('does not try to register app context when onSetAppContext is missing', () => {
    renderWithContext(<Telemetry />, {})

    expect(screen.getByTestId('dash-1')).toBeInTheDocument()
  })

  test('uses fixed positioning when navbar is hidden', () => {
    useNavbarHiddenMock.mockReturnValue({ isNavbarHidden: true })

    const { container } = renderWithContext(<Telemetry />)

    expect(container.firstChild).toHaveStyle({ position: 'fixed' })
  })

  test('wires pointer handlers from keyboard navigation hook', () => {
    const onPointerDown = jest.fn()
    const onPointerUp = jest.fn()

    useKeyboardNavigationMock.mockReturnValue({
      prev: jest.fn(),
      next: jest.fn(),
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
