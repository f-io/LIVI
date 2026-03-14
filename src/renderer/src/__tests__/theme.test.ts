import { THEME } from '../constants'
import {
  buildRuntimeTheme,
  darkTheme,
  initCursorHider,
  initUiBreatheClock,
  lightTheme
} from '../theme'

describe('theme module', () => {
  test('exports base light/dark themes', () => {
    expect(lightTheme.palette.mode).toBe('light')
    expect(darkTheme.palette.mode).toBe('dark')
  })

  test('buildRuntimeTheme applies provided primary/highlight colors', () => {
    const theme = buildRuntimeTheme(THEME.DARK, '#112233', '#aabbcc')
    expect(theme.palette.primary.main).toBe('#112233')
    expect(theme.palette.secondary.main).toBe('#aabbcc')
  })

  test('buildRuntimeTheme falls back to defaults when colors are missing', () => {
    const theme = buildRuntimeTheme(THEME.LIGHT)
    expect(theme.palette.mode).toBe('light')
    expect(typeof theme.palette.primary.main).toBe('string')
  })

  test('initCursorHider updates cursor and notifies activity', () => {
    jest.useFakeTimers()
    const notify = jest.fn()
    ;(window as any).app = { notifyUserActivity: notify }

    const main = document.createElement('div')
    main.id = 'main'
    document.body.appendChild(main)
    const btn = document.createElement('button')
    btn.className = 'MuiButtonBase-root'
    document.body.appendChild(btn)

    initCursorHider()
    expect(notify).toHaveBeenCalled()
    expect(document.body.style.cursor).toBe('default')

    jest.advanceTimersByTime(3000)
    expect(document.body.style.cursor).toBe('none')
    jest.useRealTimers()
  })

  test('initUiBreatheClock writes css variable', () => {
    jest.useFakeTimers()
    initUiBreatheClock()
    jest.advanceTimersByTime(50)
    const v = document.documentElement.style.getPropertyValue('--ui-breathe-opacity')
    expect(v).not.toBe('')
    jest.useRealTimers()
  })
})
