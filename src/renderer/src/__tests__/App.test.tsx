import { act, fireEvent, render, screen } from '@testing-library/react'
import App from '../App'
import { AppContext } from '../context'

const navigateMock = vi.fn()
const useKeyDownHandler = vi.fn()
const updateCamerasMock = vi.fn()
const listenForEvents = vi.fn()
const unlistenForEvents = vi.fn()
const focusFirstInMainMock = vi.fn()
let mockPathname = '/'

vi.mock('react-router', () => ({
  HashRouter: ({ children }: any) => <div data-testid="router">{children}</div>,
  useLocation: () => ({ pathname: mockPathname }),
  useRoutes: () => <div data-testid="routes">routes</div>,
  useNavigate: () => navigateMock
}))

vi.mock('../components/pages', () => ({
  Projection: (props: any) => <div data-testid="projection">{String(props.receivingVideo)}</div>,
  Cluster: () => <div data-testid="cluster" />,
  Home: () => <div data-testid="home" />,
  Media: () => <div data-testid="media" />,
  Camera: () => <div data-testid="camera" />,
  Devices: () => <div data-testid="devices" />,
  Maps: () => <div data-testid="maps" />,
  Telemetry: () => <div data-testid="telemetry" />
}))

vi.mock('../components/layouts/AppLayout', () => ({
  AppLayout: ({ children, navRef, mainRef }: any) => (
    <div data-testid="app-layout">
      <div data-testid="nav-slot" ref={navRef} />
      <div data-testid="main-slot" ref={mainRef}>
        {children}
      </div>
    </div>
  )
}))

vi.mock('../utils/cameraDetection', () => ({
  updateCameras: (...args: unknown[]) => updateCamerasMock(...args)
}))

vi.mock('../hooks', () => ({
  useActiveControl: () => vi.fn(),
  useFocus: () => ({
    isFormField: () => false,
    focusSelectedNav: vi.fn(),
    focusFirstInMain: focusFirstInMainMock,
    moveFocusLinear: vi.fn()
  }),
  useKeyDown: () => useKeyDownHandler
}))

const liviState: any = {
  settings: {
    startPage: 'media',
    language: 'en',
    bindings: { back: 'KeyB', selectDown: 'Enter' }
  },
  saveSettings: vi.fn()
}
const statusState: any = {
  setCameraFound: vi.fn(),
  reverse: false,
  cameraFound: false
}

vi.mock('../store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) => selector(liviState),
  useStatusStore: (selector: (s: any) => unknown) => selector(statusState)
}))

vi.mock('../utils/broadcastMediaKey', () => ({
  broadcastMediaKey: vi.fn()
}))

vi.mock('../utils/windowRole', () => ({
  getWindowRole: vi.fn(() => 'main')
}))

vi.mock('i18next', () => ({
  default: { changeLanguage: vi.fn() },
  changeLanguage: vi.fn()
}))

describe('App', () => {
  beforeEach(async () => {
    navigateMock.mockReset()
    useKeyDownHandler.mockReset()
    updateCamerasMock.mockReset()
    listenForEvents.mockReset()
    unlistenForEvents.mockReset()
    focusFirstInMainMock.mockReset()
    mockPathname = '/'
    liviState.settings = {
      startPage: 'media',
      language: 'en',
      bindings: { back: 'KeyB', selectDown: 'Enter' }
    }
    liviState.saveSettings = vi.fn()
    statusState.reverse = false
    statusState.cameraFound = false
    ;(window as any).projection = {
      usb: {
        listenForEvents,
        unlistenForEvents
      }
    }
    ;(window as any).app = undefined
  })

  test('does not redirect from configured start page when current route is not home', async () => {
    mockPathname = '/settings'
    render(<App />)

    expect(navigateMock).not.toHaveBeenCalled()
  })

  test('sets navEl and contentEl via app context when they are missing', async () => {
    const onSetAppContext = vi.fn()

    render(
      <AppContext.Provider
        value={
          {
            isTouchDevice: false,
            navEl: undefined,
            contentEl: undefined,
            onSetAppContext
          } as any
        }
      >
        <App />
      </AppContext.Provider>
    )

    expect(onSetAppContext).toHaveBeenCalledWith(
      expect.objectContaining({
        navEl: expect.any(Object),
        contentEl: expect.any(Object)
      })
    )
  })

  test('focuses first element in main after route change away from home when using keys', async () => {
    const rafSpy = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((cb: any) => {
      cb()
      return 1
    })

    mockPathname = '/'
    const { rerender } = render(
      <AppContext.Provider value={{ isTouchDevice: false } as any}>
        <App />
      </AppContext.Provider>
    )

    fireEvent.keyDown(document, { code: 'ArrowRight' })

    mockPathname = '/media'
    rerender(
      <AppContext.Provider value={{ isTouchDevice: false } as any}>
        <App />
      </AppContext.Provider>
    )

    expect(rafSpy).toHaveBeenCalled()
    expect(focusFirstInMainMock).toHaveBeenCalled()

    rafSpy.mockRestore()
  })

  test('updates cameras again for matching usb event types only', async () => {
    render(<App />)

    const usbHandler = listenForEvents.mock.calls[0][0]

    updateCamerasMock.mockClear()

    usbHandler(undefined, { type: 'attach' })
    expect(updateCamerasMock).toHaveBeenCalledTimes(1)

    updateCamerasMock.mockClear()

    usbHandler(undefined, { type: 'something-else' })
    expect(updateCamerasMock).not.toHaveBeenCalled()
  })

  test('removes global input listeners on unmount', async () => {
    const addSpy = vi.spyOn(document, 'addEventListener')
    const removeSpy = vi.spyOn(document, 'removeEventListener')

    const { unmount } = render(<App />)
    unmount()

    expect(addSpy).toHaveBeenCalledWith('pointerdown', expect.any(Function), true)
    expect(addSpy).toHaveBeenCalledWith('keydown', expect.any(Function), true)
    expect(removeSpy).toHaveBeenCalledWith('pointerdown', expect.any(Function), true)
    expect(removeSpy).toHaveBeenCalledWith('keydown', expect.any(Function), true)

    addSpy.mockRestore()
    removeSpy.mockRestore()
  })

  test('sets input mode to mouse on mouse pointerdown and to touch on non-mouse pointerdown', async () => {
    render(<App />)

    const mouseEvent = new Event('pointerdown', { bubbles: true, cancelable: true })
    Object.defineProperty(mouseEvent, 'pointerType', { value: 'mouse' })
    document.dispatchEvent(mouseEvent)

    expect(document.documentElement.dataset.input).toBe('mouse')

    const touchEvent = new Event('pointerdown', { bubbles: true, cancelable: true })
    Object.defineProperty(touchEvent, 'pointerType', { value: 'touch' })
    document.dispatchEvent(touchEvent)

    expect(document.documentElement.dataset.input).toBe('touch')
  })

  test('clears focused field in app context when focus moves away on non-touch devices', async () => {
    const onSetAppContext = vi.fn()

    render(
      <AppContext.Provider
        value={
          {
            isTouchDevice: false,
            keyboardNavigation: { focusedElId: 'focused-field' },
            onSetAppContext
          } as any
        }
      >
        <App />
      </AppContext.Provider>
    )

    onSetAppContext.mockClear()

    const other = document.createElement('button')
    other.id = 'other-field'
    document.body.appendChild(other)

    fireEvent.focusIn(other)

    expect(onSetAppContext).toHaveBeenCalledWith(
      expect.objectContaining({
        keyboardNavigation: {
          focusedElId: null
        }
      })
    )
  })

  test('sets navEl and contentEl in app context when they are missing', async () => {
    const onSetAppContext = vi.fn()

    render(
      <AppContext.Provider
        value={
          {
            isTouchDevice: false,
            navEl: null,
            contentEl: null,
            onSetAppContext
          } as any
        }
      >
        <App />
      </AppContext.Provider>
    )

    await screen.findByTestId('nav-slot')
    await screen.findByTestId('main-slot')

    expect(onSetAppContext).toHaveBeenCalledTimes(1)

    const arg = onSetAppContext.mock.calls[0][0]
    expect(arg.navEl).toEqual(
      expect.objectContaining({
        current: screen.getByTestId('nav-slot')
      })
    )
    expect(arg.contentEl).toEqual(
      expect.objectContaining({
        current: screen.getByTestId('main-slot')
      })
    )
  })

  test('window.app.onMediaKey forwards incoming commands to a car-media-key event', async () => {
    let captured: ((cmd: string) => void) | null = null
    ;(window as any).app = {
      onMediaKey: vi.fn((cb: (c: string) => void) => {
        captured = cb
        return () => {}
      })
    }
    render(<App />)
    expect((window as any).app.onMediaKey).toHaveBeenCalled()
    const listener = vi.fn()
    window.addEventListener('car-media-key', listener as never)
    captured!('next')
    expect(listener).toHaveBeenCalled()
    window.removeEventListener('car-media-key', listener as never)
  })

  test('mounts cleanly when window.app is missing', async () => {
    expect(() => render(<App />)).not.toThrow()
  })

  test('PTT keyup after keydown fires voiceAssistantRelease', async () => {
    liviState.settings = {
      ...liviState.settings,
      bindings: { ...liviState.settings.bindings, voiceAssistant: 'KeyV' }
    }
    const { broadcastMediaKey } = await vi.importMock('../utils/broadcastMediaKey')
    render(<App />)
    act(() => {
      fireEvent.keyDown(document, { code: 'KeyV' })
    })
    broadcastMediaKey.mockClear()
    act(() => {
      fireEvent.keyUp(document, { code: 'KeyV' })
    })
    expect(broadcastMediaKey).toHaveBeenCalledWith('voiceAssistantRelease')
  })

  test('PTT repeat keydown does not arm a release on a fresh press', async () => {
    liviState.settings = {
      ...liviState.settings,
      bindings: { ...liviState.settings.bindings, voiceAssistant: 'KeyV' }
    }
    const { broadcastMediaKey } = await vi.importMock('../utils/broadcastMediaKey')
    render(<App />)
    // Only repeat keydowns — never armed → keyup is a no-op
    fireEvent.keyDown(document, { code: 'KeyV', repeat: true })
    broadcastMediaKey.mockClear()
    fireEvent.keyUp(document, { code: 'KeyV' })
    expect(broadcastMediaKey).not.toHaveBeenCalled()
  })

  test('PTT release fires on window blur after a press', async () => {
    liviState.settings = {
      ...liviState.settings,
      bindings: { ...liviState.settings.bindings, voiceAssistant: 'KeyV' }
    }
    const { broadcastMediaKey } = await vi.importMock('../utils/broadcastMediaKey')
    render(<App />)
    act(() => {
      fireEvent.keyDown(document, { code: 'KeyV' })
    })
    broadcastMediaKey.mockClear()
    act(() => {
      window.dispatchEvent(new Event('blur'))
    })
    expect(broadcastMediaKey).toHaveBeenCalledWith('voiceAssistantRelease')
  })

  test('PTT release fires when document goes hidden', async () => {
    liviState.settings = {
      ...liviState.settings,
      bindings: { ...liviState.settings.bindings, voiceAssistant: 'KeyV' }
    }
    const { broadcastMediaKey } = await vi.importMock('../utils/broadcastMediaKey')
    render(<App />)
    act(() => {
      fireEvent.keyDown(document, { code: 'KeyV' })
    })
    broadcastMediaKey.mockClear()
    Object.defineProperty(document, 'visibilityState', {
      configurable: true,
      value: 'hidden'
    })
    act(() => {
      document.dispatchEvent(new Event('visibilitychange'))
    })
    expect(broadcastMediaKey).toHaveBeenCalledWith('voiceAssistantRelease')
  })

  test('reverse + camera ready auto-switches to /camera', async () => {
    liviState.settings = {
      ...liviState.settings,
      autoSwitchOnReverse: true,
      cameraId: 'cam-1',
      camera: { main: true }
    }
    statusState.reverse = true
    statusState.cameraFound = true
    mockPathname = '/media'
    render(<App />)
    expect(navigateMock).toHaveBeenCalledWith('/camera')
  })

  test('does not auto-switch when autoSwitchOnReverse is off', async () => {
    liviState.settings = {
      ...liviState.settings,
      autoSwitchOnReverse: false,
      cameraId: 'cam-1',
      camera: { main: true }
    }
    statusState.reverse = true
    statusState.cameraFound = true
    mockPathname = '/media'
    render(<App />)
    expect(navigateMock).not.toHaveBeenCalledWith('/camera')
  })

  test('does not auto-switch when the camera tab is not enabled for the role', async () => {
    liviState.settings = {
      ...liviState.settings,
      autoSwitchOnReverse: true,
      cameraId: 'cam-1',
      camera: { main: false }
    }
    statusState.reverse = true
    statusState.cameraFound = true
    mockPathname = '/media'
    render(<App />)
    expect(navigateMock).not.toHaveBeenCalledWith('/camera')
  })

  test('reverse off after an auto-switch navigates back to the prior route', async () => {
    liviState.settings = {
      ...liviState.settings,
      autoSwitchOnReverse: true,
      cameraId: 'cam-1',
      camera: { main: true }
    }
    statusState.reverse = true
    statusState.cameraFound = true
    mockPathname = '/media'
    const { rerender } = render(<App />)
    expect(navigateMock).toHaveBeenCalledWith('/camera')

    // Pretend the router has actually moved us to /camera, then reverse drops
    mockPathname = '/camera'
    statusState.reverse = false
    navigateMock.mockClear()
    rerender(<App />)
    expect(navigateMock).toHaveBeenCalledWith('/media')
  })
})
