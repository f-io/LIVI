import { fireEvent, render, screen } from '@testing-library/react'
import { ROUTES, UI } from '../../../constants'
import { Nav } from '../Nav'

const navigateMock = jest.fn()
const quitMock = jest.fn(() => Promise.resolve())

let mockPathname = '' as string
let mockIsStreaming = false
let mockTabs = [
  { label: 'Home', path: ROUTES.HOME, icon: <span>h</span> },
  { label: 'Media', path: ROUTES.MEDIA, icon: <span>m</span> },
  { label: 'Settings', path: ROUTES.SETTINGS, icon: <span>s</span> },
  { label: 'Quit', path: ROUTES.QUIT, icon: <span>q</span> }
]

jest.mock('react-router', () => {
  const actual = jest.requireActual('react-router')
  return {
    ...actual,
    useNavigate: () => navigateMock,
    useLocation: () => ({ pathname: mockPathname as ROUTES })
  }
})

jest.mock('../../../hooks/useBlinkingTime', () => ({
  useBlinkingTime: jest.fn()
}))

jest.mock('../../../hooks/useNetworkStatus', () => ({
  useNetworkStatus: jest.fn()
}))

jest.mock('../../../store/store', () => ({
  useStatusStore: (selector: (s: { isStreaming: boolean }) => unknown) =>
    selector({ isStreaming: mockIsStreaming })
}))

jest.mock('../useTabsConfig', () => ({
  useTabsConfig: () => mockTabs
}))

jest.mock('@mui/material/styles', () => ({
  useTheme: () => ({
    palette: {
      primary: {
        main: '#00aaff'
      }
    }
  })
}))

jest.mock('@mui/material/Tabs', () => ({
  __esModule: true,
  default: ({ children, onChange, value, ...props }: any) => (
    <div data-testid="tabs" data-value={value} {...props}>
      <button type="button" data-testid="tabs-change-0" onClick={(e) => onChange?.(e, 0)}>
        change-0
      </button>
      <button type="button" data-testid="tabs-change-1" onClick={(e) => onChange?.(e, 1)}>
        change-1
      </button>
      <button type="button" data-testid="tabs-change-2" onClick={(e) => onChange?.(e, 2)}>
        change-2
      </button>
      <button type="button" data-testid="tabs-change-3" onClick={(e) => onChange?.(e, 3)}>
        change-3
      </button>
      {children}
    </div>
  )
}))

jest.mock('@mui/material/Tab', () => ({
  __esModule: true,
  default: ({ icon, ...props }: any) => (
    <button type="button" {...props}>
      {icon}
    </button>
  )
}))

describe('Nav', () => {
  const originalInnerHeight = window.innerHeight

  beforeEach(() => {
    navigateMock.mockReset()
    quitMock.mockClear()

    mockPathname = ROUTES.HOME
    mockIsStreaming = false
    mockTabs = [
      { label: 'Home', path: ROUTES.HOME, icon: <span>h</span> },
      { label: 'Media', path: ROUTES.MEDIA, icon: <span>m</span> },
      { label: 'Settings', path: ROUTES.SETTINGS, icon: <span>s</span> },
      { label: 'Quit', path: ROUTES.QUIT, icon: <span>q</span> }
    ]

    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      writable: true,
      value: originalInnerHeight
    })
    ;(window as any).projection = { quit: quitMock }
  })

  test('returns null when streaming on home page', () => {
    mockIsStreaming = true
    mockPathname = ROUTES.HOME

    const { container } = render(<Nav receivingVideo={false} settings={null as never} />)

    expect(container.firstChild).toBeNull()
  })

  test('renders tabs when streaming but not on home page', () => {
    mockIsStreaming = true
    mockPathname = ROUTES.MEDIA

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toBeInTheDocument()
  })

  test('uses first tab as fallback when no active tab matches', () => {
    mockPathname = '/unknown'

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '0')
  })

  test('matches nested path with startsWith for non-home tabs', () => {
    mockPathname = '/media/library'

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '1')
  })

  test('navigates to selected tab path on tab click', () => {
    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByLabelText('Media'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.MEDIA)
  })

  test('replaces current route when clicking Settings from nested settings path', () => {
    mockPathname = '/settings/system'

    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByLabelText('Settings'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.SETTINGS, { replace: true })
  })

  test('calls projection.quit on Quit tab click', () => {
    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByLabelText('Quit'))

    expect(quitMock).toHaveBeenCalledTimes(1)
  })

  test('navigates via Tabs onChange handler', () => {
    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByTestId('tabs-change-1'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.MEDIA)
  })

  test('uses replace navigation via Tabs onChange when already inside settings', () => {
    mockPathname = '/settings/system'

    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByTestId('tabs-change-2'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.SETTINGS, { replace: true })
  })

  test('calls projection.quit via Tabs onChange for quit route', () => {
    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByTestId('tabs-change-3'))

    expect(quitMock).toHaveBeenCalledTimes(1)
  })

  test('renders with xs icon sizing branch when viewport height is small', () => {
    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      writable: true,
      value: UI.XS_ICON_MAX_HEIGHT
    })

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByLabelText('Home')).toBeInTheDocument()
  })
})
