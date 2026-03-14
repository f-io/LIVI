import { fireEvent, render, screen } from '@testing-library/react'
import App from '../App'
import { AppContext } from '../context'

const navigateMock = jest.fn()
const useKeyDownHandler = jest.fn()
const updateCamerasMock = jest.fn()
const listenForEvents = jest.fn()
const unlistenForEvents = jest.fn()
let mockPathname = '/'

jest.mock('react-router', () => ({
  HashRouter: ({ children }: any) => <div data-testid="router">{children}</div>,
  useLocation: () => ({ pathname: mockPathname }),
  useRoutes: () => <div data-testid="routes">routes</div>,
  useNavigate: () => navigateMock
}))

jest.mock('../components/pages', () => ({
  Projection: (props: any) => <div data-testid="projection">{String(props.receivingVideo)}</div>
}))

jest.mock('../components/layouts/AppLayout', () => ({
  AppLayout: ({ children }: any) => <div data-testid="app-layout">{children}</div>
}))

jest.mock('../utils/cameraDetection', () => ({
  updateCameras: (...args: unknown[]) => updateCamerasMock(...args)
}))

jest.mock('../hooks', () => ({
  useActiveControl: () => jest.fn(),
  useFocus: () => ({
    isFormField: () => false,
    focusSelectedNav: jest.fn(),
    focusFirstInMain: jest.fn(),
    moveFocusLinear: jest.fn()
  }),
  useKeyDown: () => useKeyDownHandler
}))

jest.mock('../store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: {
        startPage: 'media',
        language: 'en',
        bindings: { back: 'KeyB', selectDown: 'Enter' }
      },
      saveSettings: jest.fn()
    }),
  useStatusStore: (selector: (s: any) => unknown) =>
    selector({
      setCameraFound: jest.fn()
    })
}))

jest.mock('i18next', () => ({
  changeLanguage: jest.fn()
}))

describe('App', () => {
  beforeEach(() => {
    navigateMock.mockReset()
    useKeyDownHandler.mockReset()
    updateCamerasMock.mockReset()
    listenForEvents.mockReset()
    unlistenForEvents.mockReset()
    mockPathname = '/'
    ;(window as any).projection = {
      usb: {
        listenForEvents,
        unlistenForEvents
      }
    }
  })

  test('renders app shell and applies start page redirect', () => {
    render(<App />)
    expect(screen.getByTestId('router')).toBeInTheDocument()
    expect(screen.getByTestId('app-layout')).toBeInTheDocument()
    expect(screen.getByTestId('projection')).toBeInTheDocument()
    expect(screen.getByTestId('routes')).toBeInTheDocument()
    expect(navigateMock).toHaveBeenCalledWith('/media', { replace: true })
  })

  test('sets input mode and forwards keydown to useKeyDown handler', () => {
    render(
      <AppContext.Provider value={{ isTouchDevice: false }}>
        <App />
      </AppContext.Provider>
    )

    expect(document.documentElement.dataset.input).toBe('keys')

    fireEvent.keyDown(document, { code: 'ArrowRight' })
    expect(useKeyDownHandler).toHaveBeenCalled()
  })

  test('calls updateCameras and subscribes/unsubscribes usb listeners', () => {
    const { unmount } = render(<App />)
    expect(updateCamerasMock).toHaveBeenCalled()
    expect(listenForEvents).toHaveBeenCalled()
    unmount()
    expect(unlistenForEvents).toHaveBeenCalled()
  })
})
