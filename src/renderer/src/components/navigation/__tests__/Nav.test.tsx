import { fireEvent, render, screen } from '@testing-library/react'
import { Nav } from '../Nav'
import { ROUTES } from '../../../constants'

const navigateMock = jest.fn()
let mockPathname: string = ROUTES.HOME
let mockIsStreaming = false
const quitMock = jest.fn(() => Promise.resolve())

jest.mock('react-router', () => {
  const actual = jest.requireActual('react-router')
  return {
    ...actual,
    useNavigate: () => navigateMock,
    useLocation: () => ({ pathname: mockPathname })
  }
})

jest.mock('@renderer/hooks/useBlinkingTime', () => ({
  useBlinkingTime: () => '12:34'
}))

jest.mock('@renderer/hooks/useNetworkStatus', () => ({
  useNetworkStatus: () => ({ type: 'wifi', online: true })
}))

jest.mock('@store/store', () => ({
  useStatusStore: (selector: (s: { isStreaming: boolean }) => unknown) =>
    selector({ isStreaming: mockIsStreaming })
}))

jest.mock('../useTabsConfig', () => ({
  useTabsConfig: () => [
    { label: 'Home', path: ROUTES.HOME, icon: <span>h</span> },
    { label: 'Media', path: ROUTES.MEDIA, icon: <span>m</span> },
    { label: 'Settings', path: ROUTES.SETTINGS, icon: <span>s</span> },
    { label: 'Quit', path: ROUTES.QUIT, icon: <span>q</span> }
  ]
}))

describe('Nav', () => {
  beforeEach(() => {
    navigateMock.mockReset()
    mockPathname = ROUTES.HOME
    mockIsStreaming = false
    quitMock.mockClear()
    ;(window as any).projection = { quit: quitMock }
  })

  test('returns null when streaming on home page', () => {
    mockIsStreaming = true
    const { container } = render(<Nav receivingVideo={false} settings={null as never} />)
    expect(container.firstChild).toBeNull()
  })

  test('navigates to selected tab path', () => {
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
    expect(quitMock).toHaveBeenCalled()
  })
})
