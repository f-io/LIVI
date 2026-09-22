import { ROUTES } from '@shared/types'
import { fireEvent, render, screen } from '@testing-library/react'
import { UI } from '../../../constants'
import { Nav } from '../Nav'

const navigateMock = vi.fn()

let mockPathname = '' as string
let mockIsStreaming = false
let mockTabs = [
  { label: 'Home', path: ROUTES.HOME, icon: <span>h</span> },
  { label: 'Media', path: ROUTES.MEDIA, icon: <span>m</span> },
  { label: 'Settings', path: ROUTES.SETTINGS, icon: <span>s</span> }
]

vi.mock('react-router', async () => {
  const actual = await vi.importActual('react-router')
  return {
    ...actual,
    useNavigate: () => navigateMock,
    useLocation: () => ({ pathname: mockPathname as ROUTES })
  }
})

vi.mock('../../../hooks/useBlinkingTime', () => ({
  useBlinkingTime: vi.fn()
}))

vi.mock('../../../hooks/useNetworkStatus', () => ({
  useNetworkStatus: vi.fn()
}))

vi.mock('../../../store/store', () => ({
  useStatusStore: (selector: (s: { isStreaming: boolean }) => unknown) =>
    selector({ isStreaming: mockIsStreaming })
}))

vi.mock('../useTabsConfig', () => ({
  useTabsConfig: () => mockTabs
}))

vi.mock('@mui/material/styles', () => ({
  useTheme: () => ({
    palette: {
      primary: {
        main: '#00aaff'
      }
    }
  })
}))

vi.mock('@mui/material/Tabs', () => ({
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

vi.mock('@mui/material/Tab', () => ({
  __esModule: true,
  default: ({ icon, ...props }: any) => (
    <button type="button" {...props}>
      {icon}
    </button>
  )
}))

describe('Nav', () => {
  const originalInnerHeight = window.innerHeight

  beforeEach(async () => {
    navigateMock.mockReset()

    mockPathname = ROUTES.HOME
    mockIsStreaming = false
    mockTabs = [
      { label: 'Home', path: ROUTES.HOME, icon: <span>h</span> },
      { label: 'Media', path: ROUTES.MEDIA, icon: <span>m</span> },
      { label: 'Settings', path: ROUTES.SETTINGS, icon: <span>s</span> }
    ]

    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      writable: true,
      value: originalInnerHeight
    })
  })

  test('returns null when streaming on home page', async () => {
    mockIsStreaming = true
    mockPathname = ROUTES.HOME

    const { container } = render(<Nav receivingVideo={false} settings={null as never} />)

    expect(container.firstChild).toBeNull()
  })

  test('renders tabs when streaming but not on home page', async () => {
    mockIsStreaming = true
    mockPathname = ROUTES.MEDIA

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toBeInTheDocument()
  })

  test('uses first tab as fallback when no active tab matches', async () => {
    mockPathname = '/unknown'

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '0')
  })

  test('matches nested path with startsWith for non-home tabs', async () => {
    mockPathname = '/media/library'

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '1')
  })

  test('navigates to selected tab path on tab click', async () => {
    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByLabelText('Media'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.MEDIA)
  })

  test('falls back to the home route when there are no tabs', async () => {
    mockTabs = []
    mockPathname = '/unknown'

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByTestId('tabs')).toHaveAttribute('data-value', '0')
  })

  test('replaces current route when clicking Settings from nested settings path', async () => {
    mockPathname = '/settings/system'

    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByLabelText('Settings'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.SETTINGS, { replace: true })
  })

  test('navigates via Tabs onChange handler', async () => {
    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByTestId('tabs-change-1'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.MEDIA)
  })

  test('uses replace navigation via Tabs onChange when already inside settings', async () => {
    mockPathname = '/settings/system'

    render(<Nav receivingVideo={false} settings={null as never} />)

    fireEvent.click(screen.getByTestId('tabs-change-2'))

    expect(navigateMock).toHaveBeenCalledWith(ROUTES.SETTINGS, { replace: true })
  })

  test('renders with xs icon sizing branch when viewport height is small', async () => {
    Object.defineProperty(window, 'innerHeight', {
      configurable: true,
      writable: true,
      value: UI.XS_ICON_MAX_HEIGHT
    })

    render(<Nav receivingVideo={false} settings={null as never} />)

    expect(screen.getByLabelText('Home')).toBeInTheDocument()
  })
})
