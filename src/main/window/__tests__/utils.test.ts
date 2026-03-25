import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  attachKioskStateSync,
  currentKiosk,
  persistKioskAndBroadcast,
  restoreKioskAfterWmExit,
  sendKioskSync
} from '@main/window/utils'
import { getMainWindow } from '@main/window/createWindow'
import { isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import { saveSettings } from '@main/ipc/utils'
import { screen } from 'electron'

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: jest.fn()
}))

jest.mock('@main/utils', () => ({
  isMacPlatform: jest.fn(() => false),
  pushSettingsToRenderer: jest.fn()
}))

jest.mock('@main/ipc/utils', () => ({
  saveSettings: jest.fn()
}))

jest.mock('electron', () => ({
  screen: {
    getDisplayMatching: jest.fn(() => ({
      workAreaSize: { width: 1600, height: 900 }
    }))
  }
}))

type WindowHandler = () => void

describe('window utils', () => {
  const originalPlatform = process.platform
  const mockedGetDisplayMatching = screen.getDisplayMatching as jest.Mock

  afterEach(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    jest.clearAllMocks()
  })

  test('applyAspectRatioFullscreen sets ratio from width/height', () => {
    const win = { setAspectRatio: jest.fn() } as any
    applyAspectRatioFullscreen(win, 800, 400)
    expect(win.setAspectRatio).toHaveBeenCalledWith(2, { width: 0, height: 0 })
  })

  test('applyAspectRatioWindowed resets constraints when dimensions are missing', () => {
    const win = {
      setAspectRatio: jest.fn(),
      setMinimumSize: jest.fn()
    } as any

    applyAspectRatioWindowed(win, 0, 0)

    expect(win.setAspectRatio).toHaveBeenCalledWith(0)
    expect(win.setMinimumSize).toHaveBeenCalledWith(0, 0)
  })

  test('applyAspectRatioWindowed clears aspect ratio and sets minimum size with frame extras', () => {
    const win = {
      setAspectRatio: jest.fn(),
      setMinimumSize: jest.fn(),
      getSize: jest.fn(() => [820, 520]),
      getContentSize: jest.fn(() => [800, 480])
    } as any

    applyAspectRatioWindowed(win, 800, 480)

    expect(win.setAspectRatio).toHaveBeenCalledWith(0)
    expect(win.setMinimumSize).toHaveBeenCalledWith(320, 240)
  })

  test('applyWindowedContentSize on non-linux sets content size and reapplies windowed constraints', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const win = {
      setContentSize: jest.fn(),
      setAspectRatio: jest.fn(),
      setMinimumSize: jest.fn(),
      getSize: jest.fn(() => [820, 520]),
      getContentSize: jest.fn(() => [800, 480])
    } as any

    applyWindowedContentSize(win, 1024, 600)

    expect(win.setContentSize).toHaveBeenCalledWith(1024, 600, false)
    expect(win.setAspectRatio).toHaveBeenCalledWith(0)
    expect(win.setMinimumSize).toHaveBeenCalled()
  })

  test('applyWindowedContentSize on linux clamps size to work area and reapplies constraints', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      getBounds: jest.fn(() => ({ x: 0, y: 0, width: 800, height: 480 })),
      setResizable: jest.fn(),
      setMinimumSize: jest.fn(),
      setContentSize: jest.fn(),
      setAspectRatio: jest.fn(),
      getSize: jest.fn(() => [820, 520]),
      getContentSize: jest.fn(() => [800, 480])
    } as any

    mockedGetDisplayMatching.mockReturnValue({
      workAreaSize: { width: 1000, height: 700 }
    })

    applyWindowedContentSize(win, 1200, 800)

    expect(win.setResizable).toHaveBeenCalledWith(true)
    expect(win.setMinimumSize).toHaveBeenCalledWith(0, 0)
    expect(win.setContentSize).toHaveBeenCalledWith(1000, 700, false)
    expect(win.setAspectRatio).toHaveBeenCalledWith(0)
  })

  test('applyWindowedContentSize on linux clamps invalid sizes to at least 1x1', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      getBounds: jest.fn(() => ({ x: 0, y: 0, width: 800, height: 480 })),
      setResizable: jest.fn(),
      setMinimumSize: jest.fn(),
      setContentSize: jest.fn(),
      setAspectRatio: jest.fn(),
      getSize: jest.fn(() => [20, 20]),
      getContentSize: jest.fn(() => [20, 20])
    } as any

    mockedGetDisplayMatching.mockReturnValue({
      workAreaSize: { width: 1000, height: 700 }
    })

    applyWindowedContentSize(win, 0, -5)

    expect(win.setContentSize).toHaveBeenCalledWith(1, 1, false)
  })

  test('currentKiosk returns runtime config when main window is absent', () => {
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    expect(currentKiosk({ kiosk: true } as any)).toBe(true)
  })

  test('currentKiosk returns runtime config when window is destroyed', () => {
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => true)
    })

    expect(currentKiosk({ kiosk: false } as any)).toBe(false)
  })

  test('currentKiosk reads kiosk state from native window on non-mac', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => false),
      isKiosk: jest.fn(() => true)
    })

    expect(currentKiosk({ kiosk: false } as any)).toBe(true)
  })

  test('currentKiosk reads fullscreen state from native window on mac', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(true)
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => false),
      isFullScreen: jest.fn(() => true)
    })

    expect(currentKiosk({ kiosk: false } as any)).toBe(true)
  })

  test('persistKioskAndBroadcast only pushes when kiosk unchanged', () => {
    const runtimeState = { config: { kiosk: true }, wmExitedKiosk: true } as any

    persistKioskAndBroadcast(true, runtimeState)

    expect(pushSettingsToRenderer).toHaveBeenCalledWith(runtimeState, { kiosk: true })
    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('persistKioskAndBroadcast saves when kiosk changed', () => {
    const runtimeState = { config: { kiosk: true }, wmExitedKiosk: true } as any

    persistKioskAndBroadcast(false, runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(saveSettings).toHaveBeenCalledWith(runtimeState, { kiosk: false })
  })

  test('sendKioskSync emits kiosk sync event', () => {
    const send = jest.fn()
    sendKioskSync(true, { webContents: { send } } as any)
    expect(send).toHaveBeenCalledWith('settings:kiosk-sync', true)
  })

  test('sendKioskSync does nothing when window is null', () => {
    expect(() => sendKioskSync(true, null)).not.toThrow()
  })

  test('restoreKioskAfterWmExit returns early on non-linux', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const runtimeState = { wmExitedKiosk: true, config: { kiosk: false } } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit returns early when window is absent', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    const runtimeState = { wmExitedKiosk: true, config: { kiosk: false } } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit returns early when window is destroyed', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => true)
    })

    const runtimeState = { wmExitedKiosk: true, config: { kiosk: false } } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit returns early when kiosk was not exited by wm', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => false),
      setKiosk: jest.fn()
    })

    const runtimeState = { wmExitedKiosk: false, config: { kiosk: false } } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(saveSettings).not.toHaveBeenCalled()
  })

  test('restoreKioskAfterWmExit swallows setKiosk errors and still persists on linux', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      isDestroyed: jest.fn(() => false),
      setKiosk: jest.fn(() => {
        throw new Error('boom')
      })
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { wmExitedKiosk: true, config: { kiosk: false } } as any

    expect(() => restoreKioskAfterWmExit(runtimeState)).not.toThrow()
    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(saveSettings).toHaveBeenCalledWith(runtimeState, { kiosk: true })
  })

  test('restoreKioskAfterWmExit re-enters kiosk and persists on linux', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const win = {
      isDestroyed: jest.fn(() => false),
      setKiosk: jest.fn()
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { wmExitedKiosk: true, config: { kiosk: false } } as any

    restoreKioskAfterWmExit(runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(false)
    expect(win.setKiosk).toHaveBeenCalledWith(true)
    expect(saveSettings).toHaveBeenCalledWith(runtimeState, { kiosk: true })
  })

  test('attachKioskStateSync returns early on non-linux', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const runtimeState = { config: { kiosk: false }, wmExitedKiosk: false } as any

    expect(() => attachKioskStateSync(runtimeState)).not.toThrow()
  })

  test('attachKioskStateSync returns early when window is absent', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    const runtimeState = { config: { kiosk: false }, wmExitedKiosk: false } as any

    attachKioskStateSync(runtimeState)

    expect(pushSettingsToRenderer).not.toHaveBeenCalled()
  })

  test('attachKioskStateSync registers listeners and sends initial sync in normal mode', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: jest.fn(() => false),
      isKiosk: jest.fn(() => false),
      on: jest.fn((event: string, handler: WindowHandler) => {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { config: { kiosk: false }, wmExitedKiosk: false } as any

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

    expect(pushSettingsToRenderer).toHaveBeenCalledWith(runtimeState, { kiosk: false })
    expect(handlers.focus).toBeDefined()
  })

  test('attachKioskStateSync avoids duplicate renderer pushes for unchanged kiosk state', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: jest.fn(() => false),
      isKiosk: jest.fn(() => false),
      on: jest.fn((event: string, handler: WindowHandler) => {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { config: { kiosk: false }, wmExitedKiosk: false } as any

    attachKioskStateSync(runtimeState)
    handlers.resize()

    expect(pushSettingsToRenderer).toHaveBeenCalledTimes(1)
  })

  test('attachKioskStateSync persists truthful state when wm forces kiosk off', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: jest.fn(() => false),
      isKiosk: jest.fn(() => false),
      on: jest.fn((event: string, handler: WindowHandler) => {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { config: { kiosk: true }, wmExitedKiosk: false } as any

    attachKioskStateSync(runtimeState)

    expect(runtimeState.wmExitedKiosk).toBe(true)
    expect(saveSettings).toHaveBeenCalledWith(runtimeState, { kiosk: false })
    expect(pushSettingsToRenderer).not.toHaveBeenCalled()
  })

  test('attachKioskStateSync ignores syncs when window is destroyed', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: jest.fn(() => true),
      isKiosk: jest.fn(() => false),
      on: jest.fn((event: string, handler: WindowHandler) => {
        handlers[event] = handler
      })
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { config: { kiosk: false }, wmExitedKiosk: false } as any

    attachKioskStateSync(runtimeState)

    expect(pushSettingsToRenderer).not.toHaveBeenCalled()
    handlers.resize?.()
    expect(pushSettingsToRenderer).not.toHaveBeenCalled()
  })

  test('attachKioskStateSync restores kiosk on focus', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const handlers: Record<string, WindowHandler> = {}
    const win = {
      isDestroyed: jest.fn(() => false),
      isKiosk: jest.fn(() => false),
      on: jest.fn((event: string, handler: WindowHandler) => {
        handlers[event] = handler
      }),
      setKiosk: jest.fn()
    }
    ;(getMainWindow as jest.Mock).mockReturnValue(win)

    const runtimeState = { config: { kiosk: false }, wmExitedKiosk: true } as any

    attachKioskStateSync(runtimeState)
    handlers.focus()

    expect(win.setKiosk).toHaveBeenCalledWith(true)
    expect(saveSettings).toHaveBeenCalledWith(runtimeState, { kiosk: true })
  })
})
