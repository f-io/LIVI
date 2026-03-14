import { renderHook } from '@testing-library/react'
import { ReactNode } from 'react'
import { AppContext, AppContextProps } from '../../../context'
import { useKeyDown } from '../useKeyDown'
import { ROUTES } from '../../../constants'

const mockBroadcastMediaKey = jest.fn()

let mockPathname: string = ROUTES.HOME
let mockHash = ''
let mockSettings: any = null

jest.mock('../../../utils/broadcastMediaKey', () => ({
  broadcastMediaKey: (...args: unknown[]) => mockBroadcastMediaKey(...args)
}))

jest.mock('react-router', () => {
  const actual = jest.requireActual('react-router')
  return {
    ...actual,
    useLocation: () => ({ pathname: mockPathname, hash: mockHash })
  }
})

jest.mock('@store/store', () => ({
  useLiviStore: (selector: (s: { settings: unknown }) => unknown) =>
    selector({ settings: mockSettings })
}))

const makeEvent = (code: string) => {
  const preventDefault = jest.fn()
  const stopPropagation = jest.fn()

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
    jest.useRealTimers()
    jest.clearAllMocks()
    mockPathname = ROUTES.HOME
    mockHash = ''
    mockSettings = null
    document.body.innerHTML = ''
  })

  test('sends mapped commands in CarPlay mode and auto-emits selectUp', () => {
    jest.useFakeTimers()
    const { navRoot, contentRoot } = setupRoots()
    const mainBtn = document.createElement('button')
    contentRoot.appendChild(mainBtn)
    mainBtn.focus()

    mockSettings = {
      bindings: {
        selectDown: 'KeyS'
      }
    }

    const onSetKeyCommand = jest.fn()
    const onSetCommandCounter = jest.fn()

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      navEl: { current: navRoot },
      contentEl: { current: contentRoot },
      onSetAppContext: jest.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: true,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: jest.fn(() => true),
          focusFirstInMain: jest.fn(() => true),
          moveFocusLinear: jest.fn(() => true),
          isFormField: jest.fn(() => false),
          activateControl: jest.fn(() => true),
          onSetKeyCommand,
          onSetCommandCounter
        }),
      { wrapper }
    )

    const event = makeEvent('KeyS')
    result.current(event)

    expect(onSetKeyCommand).toHaveBeenCalledWith('selectDown')
    expect(onSetCommandCounter).toHaveBeenCalled()
    expect(mockBroadcastMediaKey).toHaveBeenCalledWith('selectDown')

    jest.advanceTimersByTime(220)

    expect(onSetKeyCommand).toHaveBeenCalledWith('selectUp')
    expect(mockBroadcastMediaKey).toHaveBeenCalledWith('selectUp')
    expect(event.preventDefault).toHaveBeenCalled()
    expect(event.stopPropagation).toHaveBeenCalled()
  })

  test('telemetry pager handles left/right when not in nav', () => {
    setupRoots()
    mockPathname = ROUTES.TELEMETRY

    const pager = {
      prev: jest.fn(),
      next: jest.fn(),
      canPrev: jest.fn(() => true),
      canNext: jest.fn(() => true)
    }

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      telemetryPager: pager,
      onSetAppContext: jest.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav: jest.fn(() => true),
          focusFirstInMain: jest.fn(() => true),
          moveFocusLinear: jest.fn(() => true),
          isFormField: jest.fn(() => false),
          activateControl: jest.fn(() => true),
          onSetKeyCommand: jest.fn(),
          onSetCommandCounter: jest.fn()
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
    mockSettings = { bindings: { selectDown: 'KeyS' } }
    const prevRaf = window.requestAnimationFrame
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      cb(0)
      return 0
    }) as typeof window.requestAnimationFrame
    const navBtn = document.createElement('button')
    navRoot.appendChild(navBtn)
    navBtn.focus()

    const dispatchSpy = jest.spyOn(navBtn, 'dispatchEvent')
    const clickSpy = jest.spyOn(navBtn, 'click')
    const activateControl = jest.fn(() => false)

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      navEl: { current: navRoot },
      onSetAppContext: jest.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const focusFirstInMain = jest.fn(() => true)

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: jest.fn(() => true),
          focusFirstInMain,
          moveFocusLinear: jest.fn(() => true),
          isFormField: jest.fn(() => false),
          activateControl,
          onSetKeyCommand: jest.fn(),
          onSetCommandCounter: jest.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowLeft'))
    result.current(makeEvent('ArrowRight'))
    result.current(makeEvent('Enter'))

    result.current(makeEvent('KeyS'))

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

    const dispatchSpy = jest.spyOn(option, 'dispatchEvent')

    const context: AppContextProps = {
      isTouchDevice: false,
      keyboardNavigation: { focusedElId: null },
      contentEl: { current: contentRoot },
      onSetAppContext: jest.fn()
    }

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={context}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: (root, el) => !!root && !!el && root.contains(el),
          focusSelectedNav: jest.fn(() => true),
          focusFirstInMain: jest.fn(() => true),
          moveFocusLinear: jest.fn(() => true),
          isFormField: jest.fn(() => false),
          activateControl: jest.fn(() => false),
          onSetKeyCommand: jest.fn(),
          onSetCommandCounter: jest.fn()
        }),
      { wrapper }
    )

    result.current(makeEvent('ArrowLeft'))
    expect(dispatchSpy).toHaveBeenCalled()
  })

  test('handles back key in settings sub-route and main interactions', () => {
    const { contentRoot } = setupRoots()

    mockPathname = '/settings/system'
    const backSpy = jest.spyOn(window.history, 'back').mockImplementation(() => undefined)

    const input = document.createElement('input')
    input.type = 'number'
    contentRoot.appendChild(input)
    input.focus()

    const onSetAppContext = jest.fn()
    const activateControl = jest.fn(() => true)
    const moveFocusLinear = jest.fn(() => true)

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
          focusSelectedNav: jest.fn(() => true),
          focusFirstInMain: jest.fn(() => true),
          moveFocusLinear,
          isFormField: (el) => !!el && el.tagName === 'INPUT',
          activateControl,
          onSetKeyCommand: jest.fn(),
          onSetCommandCounter: jest.fn()
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

    const onSetKeyCommand = jest.fn()
    const onSetCommandCounter = jest.fn()

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={{ isTouchDevice: false }}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav: jest.fn(() => true),
          focusFirstInMain: jest.fn(() => true),
          moveFocusLinear: jest.fn(() => true),
          isFormField: jest.fn(() => false),
          activateControl: jest.fn(() => false),
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
    const focusSelectedNav = jest.fn(() => true)

    const wrapper = ({ children }: { children: ReactNode }) => (
      <AppContext.Provider value={{ isTouchDevice: false }}>{children}</AppContext.Provider>
    )

    const { result } = renderHook(
      () =>
        useKeyDown({
          receivingVideo: false,
          inContainer: () => false,
          focusSelectedNav,
          focusFirstInMain: jest.fn(() => true),
          moveFocusLinear: jest.fn(() => false),
          isFormField: jest.fn(() => false),
          activateControl: jest.fn(() => false),
          onSetKeyCommand: jest.fn(),
          onSetCommandCounter: jest.fn()
        }),
      { wrapper }
    )

    const event = makeEvent('ArrowUp')
    result.current(event)

    expect(focusSelectedNav).toHaveBeenCalled()
    expect(event.preventDefault).toHaveBeenCalled()
  })
})
