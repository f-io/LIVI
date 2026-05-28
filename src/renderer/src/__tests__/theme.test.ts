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

  const pointerMove = (pointerType: string, x = 0, y = 0) => {
    const ev = new Event('pointermove') as Event & {
      pointerType?: string
      clientX?: number
      clientY?: number
    }
    Object.defineProperty(ev, 'pointerType', { value: pointerType })
    Object.defineProperty(ev, 'clientX', { value: x })
    Object.defineProperty(ev, 'clientY', { value: y })
    document.dispatchEvent(ev)
  }

  test('initCursorHider reveals pointer on real mouse movement, hides after inactivity', () => {
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
    expect(document.body.style.cursor).toBe('none')

    pointerMove('mouse', 100, 100)
    expect(document.body.style.cursor).toBe('none')

    pointerMove('mouse', 150, 150)
    expect(notify).toHaveBeenCalled()
    expect(document.body.style.cursor).toBe('default')

    jest.advanceTimersByTime(3000)
    expect(document.body.style.cursor).toBe('none')
    jest.useRealTimers()
  })

  test('initCursorHider keeps pointer hidden on touch', () => {
    const notify = jest.fn()
    ;(window as any).app = { notifyUserActivity: notify }

    initCursorHider()
    pointerMove('touch', 10, 10)
    pointerMove('touch', 50, 50)
    expect(notify).toHaveBeenCalled()
    expect(document.body.style.cursor).toBe('none')
  })

  test('initUiBreatheClock writes css variable', () => {
    jest.useFakeTimers()
    initUiBreatheClock()
    jest.advanceTimersByTime(50)
    const v = document.documentElement.style.getPropertyValue('--ui-breathe-opacity')
    expect(v).not.toBe('')
    jest.useRealTimers()
  })

  test('buildRuntimeTheme falls back to default highlight when only primary is provided', () => {
    const theme = buildRuntimeTheme(THEME.DARK, '#112233')

    expect(theme.palette.primary.main).toBe('#112233')
    expect(typeof theme.palette.secondary.main).toBe('string')
    expect(theme.palette.secondary.main).not.toBe('')
  })

  test('buildRuntimeTheme falls back to default primary when only highlight is provided', () => {
    const theme = buildRuntimeTheme(THEME.LIGHT, undefined, '#aabbcc')

    expect(theme.palette.secondary.main).toBe('#aabbcc')
    expect(typeof theme.palette.primary.main).toBe('string')
    expect(theme.palette.primary.main).not.toBe('')
  })

  test('initUiBreatheClock does nothing on second call', () => {
    jest.useFakeTimers()

    const setPropertySpy = jest.spyOn(document.documentElement.style, 'setProperty')

    initUiBreatheClock()
    const callsAfterFirstStart = setPropertySpy.mock.calls.length

    initUiBreatheClock()
    const callsAfterSecondStart = setPropertySpy.mock.calls.length

    expect(callsAfterSecondStart).toBe(callsAfterFirstStart)

    jest.useRealTimers()
  })

  test('initUiBreatheClock updates opacity across multiple animation phases', () => {
    jest.useFakeTimers()

    const perfSpy = jest.spyOn(performance, 'now')
    perfSpy
      .mockReturnValueOnce(0) // start
      .mockReturnValueOnce(100) // rising
      .mockReturnValueOnce(700) // plateau
      .mockReturnValueOnce(1100) // falling
      .mockReturnValueOnce(1500) // zero phase

    initUiBreatheClock()

    const first = document.documentElement.style.getPropertyValue('--ui-breathe-opacity')

    jest.advanceTimersByTime(42)
    const second = document.documentElement.style.getPropertyValue('--ui-breathe-opacity')

    jest.advanceTimersByTime(42)
    const third = document.documentElement.style.getPropertyValue('--ui-breathe-opacity')

    jest.advanceTimersByTime(42)
    const fourth = document.documentElement.style.getPropertyValue('--ui-breathe-opacity')

    expect(first).not.toBe('')
    expect(second).not.toBe('')
    expect(third).not.toBe('')
    expect(fourth).not.toBe('')

    perfSpy.mockRestore()
    jest.useRealTimers()
  })

  test('initUiBreatheClock covers plateau, falling and zero wave phases', () => {
    jest.resetModules()
    jest.useFakeTimers()

    const perfSpy = jest.spyOn(performance, 'now')
    perfSpy
      .mockReturnValueOnce(0) // start
      .mockReturnValueOnce(700) // p = 700 / 1600 = 0.4375  -> wave = 1
      .mockReturnValueOnce(1100) // p = 1100 / 1600 = 0.6875 -> falling branch
      .mockReturnValueOnce(1500) // p = 1500 / 1600 = 0.9375 -> wave = 0

    const { initUiBreatheClock } = require('../theme') as typeof import('../theme')

    initUiBreatheClock()

    expect(document.documentElement.style.getPropertyValue('--ui-breathe-opacity')).toBe('1.000')

    jest.advanceTimersByTime(42)
    expect(document.documentElement.style.getPropertyValue('--ui-breathe-opacity')).not.toBe(
      '1.000'
    )
    expect(document.documentElement.style.getPropertyValue('--ui-breathe-opacity')).not.toBe(
      '0.180'
    )

    jest.advanceTimersByTime(42)
    expect(document.documentElement.style.getPropertyValue('--ui-breathe-opacity')).toBe('0.180')

    perfSpy.mockRestore()
    jest.useRealTimers()
  })
})
