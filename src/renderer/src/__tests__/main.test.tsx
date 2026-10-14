import type React from 'react'

const createRootMock = vi.fn()
const renderMock = vi.fn()
const initCursorHiderMock = vi.fn()
const initUiBreatheClockMock = vi.fn()
const buildRuntimeThemeMock = vi.fn()
const setStateMock = vi.fn()

let mockedSettings: any = {
  darkMode: true,
  primaryColorDark: '#111111',
  highlightColorDark: '#222222'
}

let capturedRootElement: React.ReactElement | null = null

vi.mock('react', async () => {
  const actual = await vi.importActual('react')

  return {
    ...actual,
    useState: vi.fn((initial: unknown) => [initial, setStateMock]),
    useCallback: vi.fn((fn: unknown) => fn),
    useMemo: vi.fn((fn: () => unknown) => fn())
  }
})

vi.mock('react-dom/client', async () => {
  const createRoot = (...args: unknown[]) => {
    createRootMock(...args)
    return {
      render: (element: React.ReactElement) => {
        renderMock(element)
        capturedRootElement = element
      },
      unmount: vi.fn()
    }
  }

  return {
    __esModule: true,
    createRoot,
    default: { createRoot }
  }
})

vi.mock('../App.tsx', async () => {
  const React = await import('react')
  return {
    __esModule: true,
    default: () => React.createElement('div', { 'data-testid': 'app' }, 'app')
  }
})

vi.mock('../ClusterApp.tsx', async () => {
  const React = await import('react')
  return {
    __esModule: true,
    default: () => React.createElement('div', { 'data-testid': 'cluster-app' }, 'cluster')
  }
})

vi.mock('../AuxApp.tsx', async () => {
  const React = await import('react')
  return {
    __esModule: true,
    default: () => React.createElement('div', { 'data-testid': 'aux-app' }, 'aux')
  }
})

vi.mock('../store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: mockedSettings
    })
}))

vi.mock('../theme/theme', () => ({
  darkTheme: { palette: { mode: 'dark', source: 'darkTheme' } },
  lightTheme: { palette: { mode: 'light', source: 'lightTheme' } },
  buildRuntimeTheme: (...args: unknown[]) => buildRuntimeThemeMock(...args),
  initCursorHider: () => initCursorHiderMock(),
  initUiBreatheClock: () => initUiBreatheClockMock()
}))

vi.mock('../context', async () => {
  const React = await import('react')
  return {
    AppContext: React.createContext({
      isTouchDevice: false,
      onSetAppContext: () => {}
    })
  }
})

vi.mock('../constants', () => ({
  THEME: {
    DARK: 'dark',
    LIGHT: 'light'
  }
}))

vi.mock('@mui/material', async () => {
  const React = await import('react')
  return {
    ThemeProvider: ({ theme, children }: any) =>
      React.createElement(
        'div',
        {
          'data-testid': 'theme-provider',
          'data-theme-mode': theme?.palette?.mode,
          'data-theme-source': theme?.palette?.source ?? 'runtime'
        },
        children
      ),
    CssBaseline: ({ enableColorScheme }: any) =>
      React.createElement('div', {
        'data-testid': 'css-baseline',
        'data-enable-color-scheme': String(Boolean(enableColorScheme))
      })
  }
})

vi.mock('@fontsource/roboto/300.css', () => ({}), { virtual: true })
vi.mock('@fontsource/roboto/400.css', () => ({}), { virtual: true })
vi.mock('@fontsource/roboto/500.css', () => ({}), { virtual: true })
vi.mock('@fontsource/roboto/700.css', () => ({}), { virtual: true })
vi.mock('../i18n', () => ({}))

describe('renderer main bootstrap', () => {
  beforeEach(async () => {
    vi.resetModules()
    vi.clearAllMocks()
    capturedRootElement = null

    mockedSettings = {
      darkMode: true,
      primaryColorDark: '#111111',
      highlightColorDark: '#222222'
    }

    buildRuntimeThemeMock.mockReturnValue({
      palette: { mode: 'dark', source: 'runtime' }
    })

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: vi.fn().mockReturnValue({
        matches: false,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn()
      })
    })

    Object.defineProperty(navigator, 'maxTouchPoints', {
      configurable: true,
      value: 0
    })

    document.body.innerHTML = '<div id="root"></div>'
  })

  async function requireMain() {
    return await import('../main')
  }

  async function renderRootDirectly() {
    const mod = await requireMain()
    return mod.Root()
  }

  test('initializes UI timers and mounts react root', async () => {
    await requireMain()

    expect(initUiBreatheClockMock).toHaveBeenCalledTimes(1)
    expect(initCursorHiderMock).toHaveBeenCalledTimes(1)
    expect(createRootMock).toHaveBeenCalledWith(document.getElementById('root'))
    expect(renderMock).toHaveBeenCalledTimes(1)
    expect(capturedRootElement).toBeTruthy()
  })

  test('uses runtime theme when dark mode has color overrides', async () => {
    mockedSettings = {
      darkMode: true,
      primaryColorDark: '#111111',
      highlightColorDark: '#222222',
      backgroundColorDark: '#000000'
    }

    await renderRootDirectly()

    expect(buildRuntimeThemeMock).toHaveBeenCalledWith('dark', '#111111', '#222222', '#000000')
  })

  test('uses light runtime theme when light mode has overrides', async () => {
    mockedSettings = {
      darkMode: false,
      primaryColorLight: '#aaaaaa',
      highlightColorLight: '#bbbbbb',
      backgroundColorLight: '#d4d4d4'
    }

    buildRuntimeThemeMock.mockReturnValue({
      palette: { mode: 'light', source: 'runtime' }
    })

    await renderRootDirectly()

    expect(buildRuntimeThemeMock).toHaveBeenCalledWith('light', '#aaaaaa', '#bbbbbb', '#d4d4d4')
  })

  test('falls back to darkTheme when no overrides exist and darkMode is true', async () => {
    mockedSettings = {
      darkMode: true
    }

    await renderRootDirectly()

    expect(buildRuntimeThemeMock).not.toHaveBeenCalled()
  })

  test('falls back to lightTheme when no overrides exist and darkMode is false', async () => {
    mockedSettings = {
      darkMode: false
    }

    await renderRootDirectly()

    expect(buildRuntimeThemeMock).not.toHaveBeenCalled()
  })

  test('defaults to darkTheme when settings.darkMode is missing', async () => {
    mockedSettings = {}

    await renderRootDirectly()

    expect(buildRuntimeThemeMock).not.toHaveBeenCalled()
  })

  test('onSetAppContext merges the patch into previous app context state', async () => {
    const tree = await renderRootDirectly()

    const onSetAppContext = tree.props.value.onSetAppContext as (patch: {
      isTouchDevice?: boolean
    }) => void

    onSetAppContext({ isTouchDevice: false })

    expect(setStateMock).toHaveBeenCalledTimes(1)

    const updater = setStateMock.mock.calls[0][0] as (prev: { isTouchDevice: boolean }) => {
      isTouchDevice: boolean
    }

    expect(updater({ isTouchDevice: true })).toEqual({ isTouchDevice: false })
  })

  test('detects touch device via coarse pointer media query when maxTouchPoints is negative', async () => {
    Object.defineProperty(navigator, 'maxTouchPoints', {
      configurable: true,
      value: -1
    })

    const matchMediaMock = vi.fn().mockReturnValue({
      matches: true,
      addListener: vi.fn(),
      removeListener: vi.fn(),
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      dispatchEvent: vi.fn()
    })

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: matchMediaMock
    })

    await renderRootDirectly()

    expect(matchMediaMock).toHaveBeenCalledWith('(pointer: coarse)')
  })

  test('cluster role sets the window title and mounts ClusterApp', async () => {
    vi.doMock('../utils/windowRole', () => ({ getWindowRole: () => 'cluster' }))

    const mod = await import('../main')

    expect(document.title).toBe('Cluster')
    expect(initUiBreatheClockMock).not.toHaveBeenCalled()
    expect(initCursorHiderMock).not.toHaveBeenCalled()

    const tree = mod.Root()
    const roleApp = tree.props.children.props.children[1]
    expect(roleApp.type().props['data-testid']).toBe('cluster-app')
  })

  test('aux role sets the window title and mounts AuxApp', async () => {
    vi.doMock('../utils/windowRole', () => ({ getWindowRole: () => 'aux' }))

    const mod = await import('../main')

    expect(document.title).toBe('Auxiliary')

    const tree = mod.Root()
    const roleApp = tree.props.children.props.children[1]
    expect(roleApp.type().props['data-testid']).toBe('aux-app')
  })

  test('adds the compositor class when running under the livi compositor', async () => {
    vi.doMock('../utils/windowRole', () => ({ getWindowRole: () => 'main' }))
    ;(window as any).app = { compositor: true }
    document.documentElement.classList.remove('compositor')

    await import('../main')

    expect(document.documentElement.classList.contains('compositor')).toBe(true)
    ;(window as any).app = undefined
  })
})
