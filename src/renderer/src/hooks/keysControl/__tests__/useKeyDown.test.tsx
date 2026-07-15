import { renderHook } from '@testing-library/react'
import { ReactNode } from 'react'
import { ROUTES } from '../../../constants'
import { AppContext, AppContextProps } from '../../../context'
import { useKeyDown } from '../useKeyDown'

const mockBroadcastMediaKey = vi.fn()

let mockPathname: string = ROUTES.HOME
let mockHash = ''
let mockSettings: any = null

vi.mock('../../../utils/broadcastMediaKey', () => ({
  broadcastMediaKey: (...args: unknown[]) => mockBroadcastMediaKey(...args)
}))

vi.mock('react-router', () => ({
  useLocation: () => ({ pathname: mockPathname, hash: mockHash })
}))

vi.mock('@store/store', () => ({
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) =>
    selector({ settings: mockSettings })
}))

const makeEvent = (code: string) => {
  const preventDefault = vi.fn()
  const stopPropagation = vi.fn()

  return {
    code,
    preventDefault,
    stopPropagation
  } as unknown as KeyboardEvent
}

const setupRoots = () => {
  document.body.innerHTML = ''

  const navRoot = document.createElement('div')
  navRoot.id = 'nav-root'
  document.body.appendChild(navRoot)

  const contentRoot = document.createElement('div')
  contentRoot.id = 'content-root'
  document.body.appendChild(contentRoot)

  return { navRoot, contentRoot }
}

describe('useKeyDown', () => {
  beforeEach(() => {
    vi.useRealTimers()
    vi.clearAllMocks()
    mockPathname = ROUTES.HOME
    mockHash = ''
    mockSettings = null
    document.body.innerHTML = ''
  })

  test('sends mapped commands in CarPlay mode and auto-emits selectUp', () => {
    vi.useFakeTimers({ shouldAdvanceTime: true })
    const { navRoot, contentRoot } = setupRoots()
    const mainBtn = document.createElement('button')
    contentRoot.appendChild(mainBtn)
    mainBtn.focus()

    mockSettings = {
      bindings: {
        selectDown: 'Enter'
      }
    }

    const onSetKeyCommand = vi.fn()
    const onSetCommandCounter = vi.fn()

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      navEl: { current: navRoot },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: true,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => true),
          focusFirstInMain: vi.fn(() => true),
          moveFocusLinear: vi.fn(() => true),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => true),
          onSetKeyCommand,
          onSetCommandCounter
        }),
      { wrapper }
    )

    const event = makeEvent('Enter')
    result.current(event)

    expect(onSetKeyCommand).toHaveBeenCalledWith('selectDown')
    expect(onSetCommandCounter).toHaveBeenCalled()
    expect(mockBroadcastMediaKey).toHaveBeenCalledWith('selectDown')

    vi.advanceTimersByTime(220)

    expect(onSetKeyCommand).toHaveBeenCalledWith('selectUp')
    expect(mockBroadcastMediaKey).toHaveBeenCalledWith('selectUp')
    expect(event.preventDefault).toHaveBeenCalled()
    expect(event.stopPropagation).toHaveBeenCalled()
  })

  test('telemetry pager handles left/right when not in nav', () => {
    setupRoots()
    mockPathname = ROUTES.TELEMETRY

    const pager = {
      prev: vi.fn(),
      next: vi.fn(),
      canPrev: vi.fn(() => true),
      canNext: vi.fn(() => true)
    }

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      telemetryPager: pager,
      onSetAppContext: vi.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav: vi.fn(() => true),
          focusFirstInMain: vi.fn(() => true),
          moveFocusLinear: vi.fn(() => true),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => true),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const left = makeEvent('ArrowLeft')
    result.current(left)
    expect(pager.prev).toHaveBeenCalled()

    const right = makeEvent('ArrowRight')
    result.current(right)
    expect(pager.next).toHaveBeenCalled()
    expect(right.preventDefault).toHaveBeenCalled()
  })

  test('nav container remaps left/right to up/down and handles enter', () => {
    const { navRoot } = setupRoots()
    mockPathname = ROUTES.MEDIA
    mockSettings = { bindings: { selectDown: 'Enter' } }
    const prevRaf = window.requestAnimationFrame
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      cb(0)
      return 0
    }) as typeof window.requestAnimationFrame
    const navBtn = document.createElement('button')
    navRoot.appendChild(navBtn)
    navBtn.focus()

    const dispatchSpy = vi.spyOn(navBtn, 'dispatchEvent')
    const clickSpy = vi.spyOn(navBtn, 'click')
    const activateControl = vi.fn(() => false)

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      navEl: { current: navRoot },
      onSetAppContext: vi.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const focusFirstInMain = vi.fn(() => true)

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => true),
          focusFirstInMain,
          moveFocusLinear: vi.fn(() => true),
          isFormField: vi.fn(() => false),
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowLeft'))
    result.current(makeEvent('ArrowRight'))
    result.current(makeEvent('Enter'))

    expect(dispatchSpy).toHaveBeenCalled()
    expect(activateControl).toHaveBeenCalled()
    expect(clickSpy).toHaveBeenCalled()
    expect(focusFirstInMain).toHaveBeenCalled()

    window.requestAnimationFrame = prevRaf
  })

  test('maps listbox navigation in main area', () => {
    const { contentRoot } = setupRoots()
    const listbox = document.createElement('div')
    listbox.setAttribute('role', 'listbox')
    const option = document.createElement('button')
    option.setAttribute('role', 'menuitem')
    listbox.appendChild(option)
    contentRoot.appendChild(listbox)
    option.focus()

    const dispatchSpy = vi.spyOn(option, 'dispatchEvent')

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => true),
          focusFirstInMain: vi.fn(() => true),
          moveFocusLinear: vi.fn(() => true),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowLeft'))
    expect(dispatchSpy).toHaveBeenCalled()
  })

  test('handles back key in settings sub-route and main interactions', () => {
    const { contentRoot } = setupRoots()

    mockPathname = '/settings/system'
    const backSpy = vi.spyOn(window.history, 'back').mockImplementation(() => undefined)

    const input = document.createElement('input')
    input.type = 'number'
    contentRoot.appendChild(input)
    input.focus()

    const onSetAppContext = vi.fn()
    const activateControl = vi.fn(() => true)
    const moveFocusLinear = vi.fn(() => true)

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => true),
          focusFirstInMain: vi.fn(() => true),
          moveFocusLinear,
          isFormField: (el) => !!el && el.tagName === 'INPUT',
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('Escape'))
    expect(backSpy).toHaveBeenCalled()

    result.current(makeEvent('Enter'))
    expect(onSetAppContext).toHaveBeenCalled()
    ;(document.activeElement as HTMLInputElement).type = 'range'
    result.current(makeEvent('ArrowRight'))
    result.current(makeEvent('ArrowDown'))
    ;(document.activeElement as HTMLInputElement).type = 'text'
    result.current(makeEvent('ArrowDown'))
    expect(moveFocusLinear).toHaveBeenCalledWith(1)

    backSpy.mockRestore()
  })

  test('handles transport keys when not in CarPlay mode', () => {
    setupRoots()
    mockPathname = ROUTES.MEDIA
    mockSettings = {
      bindings: {
        next: 'MediaNext'
      }
    }

    const onSetKeyCommand = vi.fn()
    const onSetCommandCounter = vi.fn()

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={{ isTouchDevice: false }}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav: vi.fn(() => true),
          focusFirstInMain: vi.fn(() => true),
          moveFocusLinear: vi.fn(() => true),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand,
          onSetCommandCounter
        }),
      { wrapper }
    )

    result.current(makeEvent('MediaNext'))

    expect(onSetKeyCommand).toHaveBeenCalledWith('next')
    expect(onSetCommandCounter).toHaveBeenCalled()
    expect(mockBroadcastMediaKey).toHaveBeenCalledWith('next')
  })

  test('focuses nav when nothing is focused and arrow key is pressed', () => {
    setupRoots()
    const focusSelectedNav = vi.fn(() => true)

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={{ isTouchDevice: false }}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav,
          focusFirstInMain: vi.fn(() => true),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('ArrowUp')
    result.current(event)

    expect(focusSelectedNav).toHaveBeenCalled()
    expect(event.preventDefault).toHaveBeenCalled()
  })

  test('hash without leading slash is normalised to /media route', () => {
    // covers line 38: raw.startsWith('/') false branch -> `/${raw}`
    mockHash = '#media' // no leading slash after stripping '#'
    mockPathname = ''
    setupRoots()

    const focusSelectedNav = vi.fn(() => false)
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={{ isTouchDevice: false }}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav,
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    // Just need the hook to mount with the normalised route - no error means it worked
    result.current(makeEvent('ArrowUp'))
    // focusSelectedNav is called meaning the route resolved (not HOME, not blocking)
    expect(focusSelectedNav).toHaveBeenCalled()
  })

  test('Backspace in a form field without editingField returns early', () => {
    // covers lines 149-151: formFocused && !editingField && code === 'Backspace' -> return
    const { contentRoot } = setupRoots()
    const input = document.createElement('input')
    input.type = 'text'
    contentRoot.appendChild(input)
    input.focus()

    const activateControl = vi.fn(() => false)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: () => true,
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('Backspace')
    result.current(event)
    // Early return - activateControl should NOT have been called
    expect(activateControl).not.toHaveBeenCalled()
    expect(event.preventDefault).not.toHaveBeenCalled()
  })

  test('dialog root counts as inMain for focus routing', () => {
    // covers lines 107-117: !inMain && dialogRoot && active && ... -> inMain = true
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const dialog = document.createElement('div')
    dialog.setAttribute('role', 'dialog')
    const dialogBtn = document.createElement('button')
    dialog.appendChild(dialogBtn)
    document.body.appendChild(dialog)
    dialogBtn.focus()

    const activateControl = vi.fn(() => true)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    // Enter should activate the button inside the dialog (inMain=true path)
    result.current(makeEvent('Enter'))
    expect(activateControl).toHaveBeenCalled()

    document.body.removeChild(dialog)
  })

  test('Escape when editingField is set and on telemetry route clears it and focuses nav', () => {
    // covers lines 311-319: editingField set, isTelemetryRoute, not range input
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.TELEMETRY

    const btn = document.createElement('button')
    btn.id = 'some-btn'
    contentRoot.appendChild(btn)
    btn.focus()

    const focusSelectedNav = vi.fn(() => true)
    const onSetAppContext = vi.fn()
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: 'some-btn' },
      contentEl: { current: contentRoot },
      onSetAppContext
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav,
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('Escape'))
    expect(onSetAppContext).toHaveBeenCalled()
    expect(focusSelectedNav).toHaveBeenCalled()
  })

  test('handleSetFocusedElId with element having no id or aria-label sets focusedElId to null', () => {
    // covers lines 55-63: elementId === null branch
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const btn = document.createElement('button')
    // no id, no aria-label
    contentRoot.appendChild(btn)
    btn.focus()

    const onSetAppContext = vi.fn()
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: 'previous-id' },
      contentEl: { current: contentRoot },
      onSetAppContext
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: () => true,
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    // Enter on a form field with editingField='previous-id' → handleSetFocusedElId(null)
    // but active element has no id → sets focusedElId to null
    result.current(makeEvent('Enter'))
    expect(onSetAppContext).toHaveBeenCalledWith(
      expect.objectContaining({
        keyboardNavigation: { focusedElId: null }
      })
    )
  })

  test('transport keys: prev, playPause, play, pause, acceptPhone, rejectPhone, voiceAssistant', () => {
    // covers lines 487-501: full transport action chain
    setupRoots()
    mockPathname = ROUTES.MEDIA
    mockSettings = {
      bindings: {
        prev: 'MediaPrev',
        playPause: 'MediaPlayPause',
        play: 'MediaPlay',
        pause: 'MediaPause',
        acceptPhone: 'KeyA',
        rejectPhone: 'KeyR',
        voiceAssistant: 'KeyV'
      }
    }

    const onSetKeyCommand = vi.fn()
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={{ isTouchDevice: false }}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand,
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    for (const [code, expected] of [
      ['MediaPrev', 'prev'],
      ['MediaPlayPause', 'playPause'],
      ['MediaPlay', 'play'],
      ['MediaPause', 'pause'],
      ['KeyA', 'acceptPhone'],
      ['KeyR', 'rejectPhone'],
      ['KeyV', 'voiceAssistant']
    ] as const) {
      onSetKeyCommand.mockClear()
      result.current(makeEvent(code))
      expect(onSetKeyCommand).toHaveBeenCalledWith(expected)
    }
  })

  test('combobox with aria-expanded remaps rotary left/right to arrow up/down', () => {
    // covers lines 249-268: focusOnExpandedCombobox branch
    const { contentRoot } = setupRoots()
    const listbox = document.createElement('ul')
    listbox.setAttribute('role', 'listbox')
    contentRoot.appendChild(listbox)

    const combo = document.createElement('div')
    combo.setAttribute('role', 'combobox')
    combo.setAttribute('aria-expanded', 'true')
    combo.setAttribute('tabindex', '0')
    contentRoot.appendChild(combo)
    combo.focus()

    const dispatchSpy = vi.spyOn(listbox, 'dispatchEvent')

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowRight'))
    expect(dispatchSpy).toHaveBeenCalled()
  })

  test('nothing focused + arrow key on non-home route triggers focusFirstInMain', () => {
    // lines 276-287: wantEnterMainFromNothing path
    setupRoots()
    mockPathname = ROUTES.MEDIA

    const focusFirstInMain = vi.fn(() => true)

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain,
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('ArrowLeft')
    result.current(event)

    expect(focusFirstInMain).toHaveBeenCalled()
    expect(event.preventDefault).toHaveBeenCalled()
  })

  test('back key in non-settings route without editingField calls focusSelectedNav', () => {
    // lines 338-343: focusSelectedNav() fallback after back key
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.MEDIA

    const btn = document.createElement('button')
    contentRoot.appendChild(btn)
    btn.focus()

    const focusSelectedNav = vi.fn(() => true)

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav,
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('Escape')
    result.current(event)

    expect(focusSelectedNav).toHaveBeenCalled()
    expect(event.preventDefault).toHaveBeenCalled()
  })

  test('Enter on switch in main area calls activateControl', () => {
    // lines 374-386: switch/button role → activateControl
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const sw = document.createElement('button')
    sw.setAttribute('role', 'switch')
    contentRoot.appendChild(sw)
    sw.focus()

    const activateControl = vi.fn(() => true)
    const onSetAppContext = vi.fn()

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('Enter'))
    expect(activateControl).toHaveBeenCalled()
  })

  test('Enter on dropdown in main area activates and tracks focusedElId', () => {
    // lines 374-386: isDropdown → handleSetFocusedElId(active)
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const combo = document.createElement('div')
    combo.setAttribute('role', 'combobox')
    combo.setAttribute('aria-haspopup', 'listbox')
    combo.id = 'my-combo'
    combo.setAttribute('tabindex', '0')
    contentRoot.appendChild(combo)
    combo.focus()

    const activateControl = vi.fn(() => true)
    const onSetAppContext = vi.fn()

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('Enter'))
    expect(activateControl).toHaveBeenCalled()
    // dropdown path → handleSetFocusedElId records the combo id
    expect(onSetAppContext).toHaveBeenCalledWith(
      expect.objectContaining({
        keyboardNavigation: expect.objectContaining({ focusedElId: 'my-combo' })
      })
    )
  })

  test('Enter on form field with editingField set clears the editing state', () => {
    // lines 390-396: inMain + enter + formFocused + editingField → handleSetFocusedElId(null)
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const input = document.createElement('input')
    input.type = 'text'
    input.id = 'name-field'
    contentRoot.appendChild(input)
    input.focus()

    const onSetAppContext = vi.fn()
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: 'name-field' },
      contentEl: { current: contentRoot },
      onSetAppContext
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: () => true,
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('Enter'))
    expect(onSetAppContext).toHaveBeenCalledWith(
      expect.objectContaining({ keyboardNavigation: { focusedElId: null } })
    )
  })

  test('Enter on generic element in main calls activateControl fallback', () => {
    // lines 411-415: inMain + enter + not switch/form → activateControl
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const div = document.createElement('div')
    div.setAttribute('tabindex', '0')
    contentRoot.appendChild(div)
    div.focus()

    const activateControl = vi.fn(() => true)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear: vi.fn(() => false),
          isFormField: vi.fn(() => false),
          activateControl,
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('Enter'))
    expect(activateControl).toHaveBeenCalled()
  })

  test('Left on range slider in main returns early without moveFocusLinear', () => {
    // line 421: isRangeSlider && isLeft → return
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const slider = document.createElement('input')
    slider.type = 'range'
    contentRoot.appendChild(slider)
    slider.focus()

    const moveFocusLinear = vi.fn(() => true)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear,
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('ArrowLeft')
    result.current(event)
    expect(moveFocusLinear).not.toHaveBeenCalled()
    expect(event.preventDefault).not.toHaveBeenCalled()
  })

  test('Up on range slider in main calls preventDefault without moveFocusLinear navigation', () => {
    // lines 434-437: isRangeSlider && isUp → preventDefault after moveFocusLinear
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const slider = document.createElement('input')
    slider.type = 'range'
    contentRoot.appendChild(slider)
    slider.focus()

    const moveFocusLinear = vi.fn(() => false)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear,
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('ArrowUp')
    result.current(event)
    expect(event.preventDefault).toHaveBeenCalled()
  })

  test('Left in main with editingField on input returns early (skips moveFocusLinear)', () => {
    // lines 424-430: !isRangeSlider && editingField && isInputOrEditable → return
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const input = document.createElement('input')
    input.type = 'text'
    input.id = 'edit-field'
    contentRoot.appendChild(input)
    input.focus()

    const moveFocusLinear = vi.fn(() => true)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: 'edit-field' },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear,
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowLeft'))
    expect(moveFocusLinear).not.toHaveBeenCalled()
  })

  test('Right in main with editingField on input returns early (skips moveFocusLinear)', () => {
    // line 457: !isRangeSlider && editingField && isInputOrEditable → return
    const { contentRoot } = setupRoots()
    mockPathname = ROUTES.SETTINGS

    const input = document.createElement('input')
    input.type = 'text'
    input.id = 'edit-field-r'
    contentRoot.appendChild(input)
    input.focus()

    const moveFocusLinear = vi.fn(() => true)
    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: 'edit-field-r' },
      contentEl: { current: contentRoot },
      onSetAppContext: vi.fn()
    }
    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: vi.fn(() => false),
          focusFirstInMain: vi.fn(() => false),
          moveFocusLinear,
          isFormField: vi.fn(() => false),
          activateControl: vi.fn(() => false),
          onSetKeyCommand: vi.fn(),
          onSetCommandCounter: vi.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowRight'))
    expect(moveFocusLinear).not.toHaveBeenCalled()
  })
})
