import { saveConfig } from '@main/core'
import { getMainWindow } from '@main/window/createWindow'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  attachKioskStateSync,
  attachResizeReflow,
  fitToShape,
  persistKiosk,
  restoreKioskAfterWmExit,
  sanitizeBounds,
  uiZoomFactor
} from '@main/window/utils'
import { screen } from 'electron'
import type { Mock } from 'vitest'

vi.mock('@main/window/createWindow', () => ({
  getMainWindow: vi.fn()
}))

vi.mock('@main/core', () => ({
  saveConfig: vi.fn()
}))

vi.mock('electron', () => ({
  screen: {
    getDisplayMatching: vi.fn(function () {
      return {
        workAreaSize: { width: 1600, height: 900 }
      }
    }),
    getAllDisplays: vi.fn(() => [])
  }
}))

type WindowHandler = () => void

describe('window utils', () => {
  const originalPlatform = process.platform
  const mockedGetDisplayMatching = screen.getDisplayMatching as Mock

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    vi.clearAllMocks()
  })

  test('applyAspectRatioFullscreen sets ratio from width/height', () => {
    const win = { setAspectRatio: vi.fn() } as any
    applyAspectRatioFullscreen(win, 800, 400)
    expect(win.setAspectRatio).toHaveBeenCalledWith(2, { width: 0, height: 0 })
  })

  test('applyAspectRatioFullscreen clears the ratio when a dimension is missing', () => {
    const win = { setAspectRatio: vi.fn() } as any
    applyAspectRatioFullscreen(win, 0, 480)
    expect(win.setAspectRatio).toHaveBeenCalledWith(0, { width: 0, height: 0 })
  })

  test('applyAspectRatioWindowed resets constraints when dimensions are missing', () => {
    const win = {
      setAspectRatio: vi.fn(),
      setMinimumSize: vi.fn()
    } as any

    applyAspectRatioWindowed(win, 0, 0)

    expect(win.setAspectRatio).toHaveBeenCalledWith(0)
    expect(win.setMinimumSize).toHaveBeenCalledWith(0, 0)
  })

  test('applyAspectRatioWindowed keeps the configured shape and sets minimum size with frame extras', () => {
    const win = {
      setAspectRatio: vi.fn(),
      setMinimumSize: vi.fn(),
      getSize: vi.fn(() => [820, 520]),
      getContentSize: vi.fn(() => [800, 480])
    } as any

    applyAspectRatioWindowed(win, 800, 480)

    expect(win.setAspectRatio).toHaveBeenCalledWith(800 / 480, { width: 0, height: 0 })
    expect(win.setMinimumSize).toHaveBeenCalledWith(320, 240)
  })

  test('applyWindowedContentSize on non-linux sets content size and reapplies windowed constraints', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const win = {
      getBounds: vi.fn(() => ({ x: 0, y: 0, width: 820, height: 520 })),
      setContentSize: vi.fn(),
      setAspectRatio: vi.fn(),
      setMinimumSize: vi.fn(),
      getSize: vi.fn(() => [820, 520]),
      getContentSize: vi.fn(() => [800, 480])
    } as any

    applyWindowedContentSize(win, 1024, 600)

    expect(win.setContentSize).toHaveBeenCalledWith(1024, 600, false)
    expect(win.setAspectRatio).toHaveBeenCalledWith(1024 / 600, { width: 0, height: 0 })
    expect(win.setMinimumSize).toHaveBeenCalled()
  })

  test('applyWindowedContentSize keeps the configured shape for a saved size and fits the screen', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    mockedGetDisplayMatching.mockReturnValue({ workAreaSize: { width: 1600, height: 900 } })

    const win = {
      getBounds: vi.fn(() => ({ x: 0, y: 0, width: 820, height: 520 })),
      setContentSize: vi.fn(),
      setAspectRatio: vi.fn(),
      setMinimumSize: vi.fn(),
      getSize: vi.fn(() => [820, 520]),
      getContentSize: vi.fn(() => [800, 480])
    } as any

    applyWindowedContentSize(win, 1920, 1080, { width: 1846, height: 1154 })

    expect(win.setContentSize).toHaveBeenCalledWith(1529, 860, false)
    expect(win.setAspectRatio).toHaveBeenCalledWith(1920 / 1080, { width: 0, height: 0 })
  })

  test('fitToShape keeps the wanted width in the shape and shrinks the whole to the room', () => {
    const shape = { width: 1920, height: 1080 }
    expect(fitToShape({ width: 1280, height: 720 }, { width: 1280, height: 720 }, shape)).toEqual({
      width: 1280,
      height: 720
    })
    expect(fitToShape({ width: 1846, height: 1154 }, shape, { width: 4000, height: 4000 })).toEqual(
      { width: 1846, height: 1038 }
    )
    expect(fitToShape(shape, shape, { width: 1680, height: 1017 })).toEqual({
      width: 1680,
      height: 945
    })
    expect(fitToShape({ width: 0, height: 0 }, shape, { width: 0, height: 0 })).toEqual(shape)
  })

  test('fitToShape without a shape keeps the wanted size, at least 1x1', () => {
    const none = { width: 0, height: 0 }
    expect(fitToShape({ width: 800, height: 600 }, none, none)).toEqual({ width: 800, height: 600 })
    expect(fitToShape({ width: 0, height: -5 }, none, none)).toEqual({ width: 1, height: 1 })
  })

  test('applyWindowedContentSize on linux fits the shape into the work area and reapplies constraints', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      getBounds: vi.fn(function () {
        return { x: 0, y: 0, width: 800, height: 480 }
      }),
      setResizable: vi.fn(),
      setMinimumSize: vi.fn(),
      setContentSize: vi.fn(),
      setAspectRatio: vi.fn(),
      getSize: vi.fn(() => [820, 520]),
      getContentSize: vi.fn(() => [800, 480])
    } as any

    mockedGetDisplayMatching.mockReturnValue({
      workAreaSize: { width: 1000, height: 700 }
    })

    applyWindowedContentSize(win, 1200, 800)

    expect(win.setResizable).toHaveBeenCalledWith(true)
    expect(win.setMinimumSize).toHaveBeenCalledWith(0, 0)
    // 1000 x 700 less the frame leaves 980 x 660, the 3:2 shape fits as 980 x 653.
    expect(win.setContentSize).toHaveBeenCalledWith(980, 653, false)
    expect(win.setAspectRatio).toHaveBeenCalledWith(1200 / 800, { width: 0, height: 0 })
  })

  test('applyWindowedContentSize on linux clamps invalid sizes to at least 1x1', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      getBounds: vi.fn(function () {
        return { x: 0, y: 0, width: 800, height: 480 }
      }),
      setResizable: vi.fn(),
      setMinimumSize: vi.fn(),
      setContentSize: vi.fn(),
      setAspectRatio: vi.fn(),
      getSize: vi.fn(() => [20, 20]),
      getContentSize: vi.fn(() => [20, 20])
    } as any

    mockedGetDisplayMatching.mockReturnValue({
      workAreaSize: { width: 1000, height: 700 }
    })

    applyWindowedContentSize(win, 0, -5)

    expect(win.setContentSize).toHaveBeenCalledWith(1, 1, false)
  })

  test('persistKiosk leaves an unchanged kiosk alone', () => {
    const runtimeState = {
      config: { kiosk: { main: true, cluster: false, aux: false } },
      wmExitedKiosk: true
    } as any

    persistKiosk(true, runtimeState)

    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('persistKiosk saves when kiosk changed', () => {
    const runtimeState = {
      config: { kiosk: { main: true, cluster: false, aux: false } },
      wmExitedKiosk: true
    } as any

    persistKiosk(false, runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(saveConfig).toHaveBeenCalledWith({
      kiosk: { main: false, cluster: false, aux: false }
    })
  })

  test('persistKiosk takes a config without kiosk flags as off', () => {
    const runtimeState = { config: {}, wmExitedKiosk: false } as any

    persistKiosk(false, runtimeState)
    expect(saveConfig).not.toHaveBeenCalled()

    persistKiosk(true, runtimeState)
    expect(saveConfig).toHaveBeenCalledWith({ kiosk: { main: true, cluster: false, aux: false } })
  })

  test('restoreKioskAfterWmExit returns early on non-linux', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const runtimeState = {
      wmExitedKiosk: true,
      config: { kiosk: { main: false, cluster: false, aux: false } }
    } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit returns early when window is absent', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as Mock).mockReturnValue(null)

    const runtimeState = {
      wmExitedKiosk: true,
      config: { kiosk: { main: false, cluster: false, aux: false } }
    } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit returns early when window is destroyed', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as Mock).mockReturnValue({
      isDestroyed: vi.fn(() => true)
    })

    const runtimeState = {
      wmExitedKiosk: true,
      config: { kiosk: { main: false, cluster: false, aux: false } }
    } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit returns early when kiosk was not exited by wm', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as Mock).mockReturnValue({
      isDestroyed: vi.fn(() => false),
      setKiosk: vi.fn()
    })

    const runtimeState = {
      wmExitedKiosk: false,
      config: { kiosk: { main: false, cluster: false, aux: false } }
    } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit swallows setKiosk errors and still persists on linux', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      isDestroyed: vi.fn(() => false),
      setKiosk: vi.fn(function () {
        throw new Error('boom')
      })
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      wmExitedKiosk: true,
      config: { kiosk: { main: false, cluster: false, aux: false } }
    } as any

    expect(() => restoreKioskAfterWmExit(runtimeState)).not.toThrow()
    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(saveConfig).toHaveBeenCalledWith({
      kiosk: { main: true, cluster: false, aux: false }
    })
  })

  test('restoreKioskAfterWmExit re-enters kiosk and persists on linux', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      isDestroyed: vi.fn(() => false),
      setKiosk: vi.fn()
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      wmExitedKiosk: true,
      config: { kiosk: { main: false, cluster: false, aux: false } }
    } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(win.setKiosk).toHaveBeenCalledWith(true)
    expect(saveConfig).toHaveBeenCalledWith({
      kiosk: { main: true, cluster: false, aux: false }
    })
  })

  test('attachKioskStateSync returns early on non-linux', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    expect(() => attachKioskStateSync(runtimeState)).not.toThrow()
  })

  test('attachKioskStateSync returns early when window is absent', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as Mock).mockReturnValue(null)

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)

    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('attachKioskStateSync registers listeners and sends initial sync in normal mode', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: vi.fn(() => false),
      isKiosk: vi.fn(() => false),
      on: vi.fn(function (event: string, handler: WindowHandler) {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)

    expect(win.on).toHaveBeenCalledWith('enter-full-screen', expect.anything())
    expect(win.on).toHaveBeenCalledWith('leave-full-screen', expect.anything())
    expect(win.on).toHaveBeenCalledWith('resize', expect.anything())
    expect(win.on).toHaveBeenCalledWith('move', expect.anything())
    expect(win.on).toHaveBeenCalledWith('show', expect.anything())
    expect(win.on).toHaveBeenCalledWith('focus', expect.anything())
    expect(win.on).toHaveBeenCalledWith('blur', expect.anything())
    expect(win.on).toHaveBeenCalledWith('restore', expect.anything())
    expect(win.on).toHaveBeenCalledWith('minimize', expect.anything())

    expect(saveConfig).not.toHaveBeenCalled()
    expect(handlers.focus).toBeDefined()
  })

  test('attachKioskStateSync saves a kiosk change once', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: vi.fn(() => false),
      isKiosk: vi.fn(() => false),
      on: vi.fn(function (event: string, handler: WindowHandler) {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: true, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)
    handlers.resize()

    expect(saveConfig).toHaveBeenCalledTimes(1)
  })

  test('attachKioskStateSync persists truthful state when wm forces kiosk off', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: vi.fn(() => false),
      isKiosk: vi.fn(() => false),
      on: vi.fn(function (event: string, handler: WindowHandler) {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: true, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(true)
    expect(saveConfig).toHaveBeenCalledWith({
      kiosk: { main: false, cluster: false, aux: false }
    })
  })

  test('attachKioskStateSync ignores syncs when window is destroyed', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: vi.fn(() => true),
      isKiosk: vi.fn(() => false),
      on: vi.fn(function (event: string, handler: WindowHandler) {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)

    handlers.resize?.()
    expect(saveConfig).not.toHaveBeenCalled()
  })

  test('attachKioskStateSync reads fullscreen state in compositor mode', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.LIVI_COMPOSITOR = '1'

    const win = {
      isDestroyed: vi.fn(() => false),
      isFullScreen: vi.fn(() => false),
      isKiosk: vi.fn(() => false),
      on: vi.fn()
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)

    expect(win.isFullScreen).toHaveBeenCalled()
    expect(win.isKiosk).not.toHaveBeenCalled()
    expect(saveConfig).not.toHaveBeenCalled()
    delete process.env.LIVI_COMPOSITOR
  })

  test('attachKioskStateSync persists kiosk entered from outside electron', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      isDestroyed: vi.fn(() => false),
      isKiosk: vi.fn(() => true),
      on: vi.fn()
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: false
    } as any

    attachKioskStateSync(runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(saveConfig).toHaveBeenCalledWith({
      kiosk: { main: true, cluster: false, aux: false }
    })
  })

  test('attachKioskStateSync restores kiosk on focus', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: vi.fn(() => false),
      isKiosk: vi.fn(() => false),
      on: vi.fn(function (event: string, handler: WindowHandler) {
        handlers[event] = handler
      }),
      setKiosk: vi.fn()
    }
    ;(getMainWindow as Mock).mockReturnValue(win)

    const runtimeState = {
      config: { kiosk: { main: false, cluster: false, aux: false } },
      wmExitedKiosk: true
    } as any

    attachKioskStateSync(runtimeState)
    handlers.focus()

    expect(win.setKiosk).toHaveBeenCalledWith(true)
    expect(saveConfig).toHaveBeenCalledWith({
      kiosk: { main: true, cluster: false, aux: false }
    })
  })

  describe('sanitizeBounds', () => {
    const mockedGetAllDisplays = screen.getAllDisplays as Mock

    const display = (x: number, y: number, width: number, height: number) => ({
      workArea: { x, y, width, height }
    })

    test('returns undefined for missing or malformed bounds', () => {
      mockedGetAllDisplays.mockReturnValue([display(0, 0, 1920, 1080)])
      expect(sanitizeBounds(undefined)).toBeUndefined()
      expect(sanitizeBounds({ x: 0, y: 0, width: 0, height: 480 } as any)).toBeUndefined()
      expect(sanitizeBounds({ x: 0, y: 0, width: 800 } as any)).toBeUndefined()
    })

    test('keeps a rect that is visible on a display', () => {
      mockedGetAllDisplays.mockReturnValue([display(0, 0, 1920, 1080)])
      const b = { x: 100, y: 100, width: 800, height: 480 }
      expect(sanitizeBounds(b)).toBe(b)
    })

    test('keeps a rect on a second monitor (negative offset)', () => {
      mockedGetAllDisplays.mockReturnValue([
        display(0, 0, 1920, 1080),
        display(-1920, 0, 1920, 1080)
      ])
      const b = { x: -1820, y: 200, width: 800, height: 480 }
      expect(sanitizeBounds(b)).toBe(b)
    })

    test('drops a rect that lies fully off all displays (monitor unplugged)', () => {
      mockedGetAllDisplays.mockReturnValue([display(0, 0, 1920, 1080)])
      expect(sanitizeBounds({ x: -1820, y: 200, width: 800, height: 480 })).toBeUndefined()
    })

    test('drops a rect with only a sliver visible (< 64px)', () => {
      mockedGetAllDisplays.mockReturnValue([display(0, 0, 1920, 1080)])
      expect(sanitizeBounds({ x: 1910, y: 200, width: 800, height: 480 })).toBeUndefined()
    })

    test('trusts the rect when no display info is available (headless)', () => {
      mockedGetAllDisplays.mockReturnValue([])
      const b = { x: 4000, y: 4000, width: 800, height: 480 }
      expect(sanitizeBounds(b)).toBe(b)
    })

    test('trusts the rect when the screen api lacks getAllDisplays', () => {
      const original = screen.getAllDisplays
      ;(screen as any).getAllDisplays = undefined
      const b = { x: 1, y: 2, width: 800, height: 480 }
      expect(sanitizeBounds(b)).toBe(b)
      ;(screen as any).getAllDisplays = original
    })
  })

  describe('attachResizeReflow', () => {
    const mockedGetMainWindow = getMainWindow as Mock

    function fakeWin() {
      const handlers: Record<string, () => void> = {}
      return {
        handlers,
        on: vi.fn((ev: string, cb: () => void) => {
          handlers[ev] = cb
        }),
        isDestroyed: vi.fn(() => false),
        isFullScreen: vi.fn(() => false),
        getContentSize: vi.fn(() => [800, 480]),
        setContentSize: vi.fn()
      }
    }

    afterEach(() => {
      delete process.env.LIVI_COMPOSITOR
    })

    test('is a no-op outside the compositor', () => {
      delete process.env.LIVI_COMPOSITOR
      const win = fakeWin()
      mockedGetMainWindow.mockReturnValue(win)
      attachResizeReflow()
      expect(win.on).not.toHaveBeenCalled()
    })

    test('is a no-op when there is no main window', () => {
      process.env.LIVI_COMPOSITOR = '1'
      mockedGetMainWindow.mockReturnValue(null)
      expect(() => attachResizeReflow()).not.toThrow()
    })

    test('nudges the content size after a resize settles, then restores it', () => {
      vi.useFakeTimers()
      process.env.LIVI_COMPOSITOR = '1'
      const win = fakeWin()
      mockedGetMainWindow.mockReturnValue(win)
      attachResizeReflow()

      win.handlers.resize()
      vi.advanceTimersByTime(200)
      expect(win.setContentSize).toHaveBeenCalledWith(800, 481)

      vi.advanceTimersByTime(60)
      expect(win.setContentSize).toHaveBeenLastCalledWith(800, 480)

      vi.advanceTimersByTime(60)
      vi.useRealTimers()
    })

    test('debounces a burst of resizes into a single nudge', () => {
      vi.useFakeTimers()
      process.env.LIVI_COMPOSITOR = '1'
      const win = fakeWin()
      mockedGetMainWindow.mockReturnValue(win)
      attachResizeReflow()

      win.handlers.resize()
      win.handlers.resize()
      win.handlers.resize()
      vi.advanceTimersByTime(200)

      expect(win.setContentSize).toHaveBeenCalledTimes(1)
      vi.useRealTimers()
    })

    test('skips the nudge when the window is fullscreen when the timer fires', () => {
      vi.useFakeTimers()
      process.env.LIVI_COMPOSITOR = '1'
      const win = fakeWin()
      win.isFullScreen.mockReturnValue(true)
      mockedGetMainWindow.mockReturnValue(win)
      attachResizeReflow()

      win.handlers.resize()
      vi.advanceTimersByTime(200)
      expect(win.setContentSize).not.toHaveBeenCalled()
      vi.useRealTimers()
    })

    test('skips the restore when the window is destroyed mid-nudge', () => {
      vi.useFakeTimers()
      process.env.LIVI_COMPOSITOR = '1'
      const win = fakeWin()
      mockedGetMainWindow.mockReturnValue(win)
      attachResizeReflow()

      win.handlers.resize()
      vi.advanceTimersByTime(200)
      expect(win.setContentSize).toHaveBeenCalledWith(800, 481)

      win.isDestroyed.mockReturnValue(true)
      vi.advanceTimersByTime(60)
      expect(win.setContentSize).toHaveBeenCalledTimes(1)

      vi.advanceTimersByTime(60)
      vi.useRealTimers()
    })

    test('ignores a resize on a destroyed window', () => {
      process.env.LIVI_COMPOSITOR = '1'
      const win = fakeWin()
      win.isDestroyed.mockReturnValue(true)
      mockedGetMainWindow.mockReturnValue(win)
      attachResizeReflow()

      expect(() => win.handlers.resize()).not.toThrow()
      expect(win.setContentSize).not.toHaveBeenCalled()
    })
  })
})

describe('uiZoomFactor', () => {
  test('keeps the plain percent on normal-height windows', () => {
    expect(uiZoomFactor(100, 720)).toBe(1)
    expect(uiZoomFactor(125, 720)).toBe(1.25)
    expect(uiZoomFactor(undefined, 480)).toBe(1)
  })

  test('shrinks the base zoom below the minimum UI viewport height', () => {
    expect(uiZoomFactor(100, 234)).toBeCloseTo(234 / 292, 5)
    expect(uiZoomFactor(50, 234)).toBeCloseTo(0.5 * (234 / 292), 5)
  })

  test('falls back to base 1 when the height is unknown', () => {
    expect(uiZoomFactor(100, 0)).toBe(1)
  })
})
