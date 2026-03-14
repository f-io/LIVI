const createRootMock = jest.fn((..._args: unknown[]) => ({ render: jest.fn() }))
const initCursorHiderMock = jest.fn()
const initUiBreatheClockMock = jest.fn()

jest.mock('react-dom/client', () => ({
  __esModule: true,
  default: { createRoot: (...args: unknown[]) => createRootMock(...args) }
}))

jest.mock('../App', () => ({
  __esModule: true,
  default: () => null
}))

jest.mock('../store/store', () => ({
  useLiviStore: (selector: (s: any) => unknown) =>
    selector({
      settings: {
        nightMode: true,
        primaryColorDark: '#111111',
        highlightColorDark: '#222222'
      }
    })
}))

jest.mock('../theme', () => ({
  darkTheme: { palette: { mode: 'dark' } },
  lightTheme: { palette: { mode: 'light' } },
  buildRuntimeTheme: jest.fn(() => ({ palette: { mode: 'dark' } })),
  initCursorHider: () => initCursorHiderMock(),
  initUiBreatheClock: () => initUiBreatheClockMock()
}))

jest.mock('@fontsource/roboto/300.css', () => ({}), { virtual: true })
jest.mock('@fontsource/roboto/400.css', () => ({}), { virtual: true })
jest.mock('@fontsource/roboto/500.css', () => ({}), { virtual: true })
jest.mock('@fontsource/roboto/700.css', () => ({}), { virtual: true })
jest.mock('../i18n', () => ({}))

describe('renderer main bootstrap', () => {
  test('initializes UI timers and mounts react root', async () => {
    document.body.innerHTML = '<div id="root"></div>'
    await import('../main')

    expect(initUiBreatheClockMock).toHaveBeenCalled()
    expect(initCursorHiderMock).toHaveBeenCalled()
    expect(createRootMock).toHaveBeenCalledWith(document.getElementById('root'))
  })
})
