import type React from 'react'

const createRootMock = jest.fn()
const renderMock = jest.fn()
const initCursorHiderMock = jest.fn()
const initUiBreatheClockMock = jest.fn()
const buildRuntimeThemeMock = jest.fn()
const setStateMock = jest.fn()

let mockedSettings: any = {
  nightMode: true,
  primaryColorDark: '#111111',
  highlightColorDark: '#222222'
}

let capturedRootElement: React.ReactElement | null = null

jest.mock('react', () => {
  const actual = jest.requireActual('react')

  return {
    ...actual,
    useState: jest.fn((initial: unknown) => [initial, setStateMock]),
    useCallback: jest.fn((fn: unknown) => fn),
    useMemo: jest.fn((fn: () => unknown) => fn())
  }
})

jest.mock('react-dom/client', () => {
  const createRoot = (...args: unknown[]) => {
    createRootMock(...args)
    return {
      render: (element: React.ReactElement) => {
        renderMock(element)
        capturedRootElement = element
      },
      unmount: jest.fn()
    }
  }

  return {
    __esModule: true,
    createRoot,
    default: { createRoot }
  }
})

jest.mock('../App.tsx', () => ({
  __esModule: true,
  default: () => {
    const React = require('react')
    return React.createElement('div', { 'data-testid': 'app' }, 'app')
  }
}))

jest.mock('../store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: mockedSettings
    })
}))

jest.mock('../theme', () => ({
  darkTheme: { palette: { mode: 'dark', source: 'darkTheme' } },
  lightTheme: { palette: { mode: 'light', source: 'lightTheme' } },
  buildRuntimeTheme: (...args: unknown[]) => buildRuntimeThemeMock(...args),
  initCursorHider: () => initCursorHiderMock(),
  initUiBreatheClock: () => initUiBreatheClockMock()
}))

jest.mock('../context', () => {
  const React = require('react')
  return {
    AppContext: React.createContext({
      isTouchDevice: false,
      onSetAppContext: () => {}
    })
  }
})

jest.mock('../constants', () => ({
  THEME: {
    DARK: 'dark',
    LIGHT: 'light'
  }
}))

jest.mock('@mui/material', () => {
  const React = require('react')
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

jest.mock('@fontsource/roboto/300.css', () => ({}), { virtual: true })
jest.mock('@fontsource/roboto/400.css', () => ({}), { virtual: true })
jest.mock('@fontsource/roboto/500.css', () => ({}), { virtual: true })
jest.mock('@fontsource/roboto/700.css', () => ({}), { virtual: true })
jest.mock('../i18n', () => ({}))

describe('renderer main bootstrap', () => {
  beforeEach(() => {
    jest.resetModules()
    jest.clearAllMocks()
    capturedRootElement = null

    mockedSettings = {
      nightMode: true,
      primaryColorDark: '#111111',
      highlightColorDark: '#222222'
    }

    buildRuntimeThemeMock.mockReturnValue({
      palette: { mode: 'dark', source: 'runtime' }
    })

    Object.defineProperty(window, 'matchMedia', {
      writable: true,
      value: jest.fn().mockReturnValue({
        matches: false,
        addListener: jest.fn(),
        removeListener: jest.fn(),
        addEventListener: jest.fn(),
        removeEventListener: jest.fn(),
        dispatchEvent: jest.fn()
      })
    })

    Object.defineProperty(navigator, 'maxTouchPoints', {
      configurable: true,
      value: 0
    })

    document.body.innerHTML = '<div id="root"></div>'
  })

  function requireMain() {
    return require('../main') as typeof import('../main')
  }

  function renderRootDirectly() {
    const mod = requireMain()
    return mod.Root()
  }

  test('initializes UI timers and mounts react root', () => {
    requireMain()

    expect(initUiBreatheClockMock).toHaveBeenCalledTimes(1)
    expect(initCursorHiderMock).toHaveBeenCalledTimes(1)
    expect(createRootMock).toHaveBeenCalledWith(document.getElementById('root'))
    expect(renderMock).toHaveBeenCalledTimes(1)
    expect(capturedRootElement).toBeTruthy()
  })

  test('uses runtime theme when dark mode has color overrides', () => {
    mockedSettings = {
      nightMode: true,
      primaryColorDark: '#111111',
      highlightColorDark: '#222222'
    }

    renderRootDirectly()

    expect(buildRuntimeThemeMock).toHaveBeenCalledWith('dark', '#111111', '#222222')
  })

  test('uses light runtime theme when light mode has overrides', () => {
    mockedSettings = {
      nightMode: false,
      primaryColorLight: '#aaaaaa',
      highlightColorLight: '#bbbbbb'
    }

    buildRuntimeThemeMock.mockReturnValue({
      palette: { mode: 'light', source: 'runtime' }
    })

    renderRootDirectly()

    expect(buildRuntimeThemeMock).toHaveBeenCalledWith('light', '#aaaaaa', '#bbbbbb')
  })

  test('falls back to darkTheme when no overrides exist and nightMode is true', () => {
    mockedSettings = {
      nightMode: true
    }

    renderRootDirectly()

    expect(buildRuntimeThemeMock).not.toHaveBeenCalled()
  })

  test('falls back to lightTheme when no overrides exist and nightMode is false', () => {
    mockedSettings = {
      nightMode: false
    }

    renderRootDirectly()

    expect(buildRuntimeThemeMock).not.toHaveBeenCalled()
  })

  test('defaults to darkTheme when settings.nightMode is missing', () => {
    mockedSettings = {}

    renderRootDirectly()

    expect(buildRuntimeThemeMock).not.toHaveBeenCalled()
  })
})
