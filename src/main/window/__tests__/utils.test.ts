import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  currentKiosk,
  persistKioskAndBroadcast,
  restoreKioskAfterWmExit,
  sendKioskSync
} from '@main/window/utils'
import { getMainWindow } from '@main/window/createWindow'
import { isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import { saveSettings } from '@main/ipc/utils'

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

describe('window utils', () => {
  const originalPlatform = process.platform

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

  test('currentKiosk returns runtime config when main window is absent', () => {
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    expect(currentKiosk({ kiosk: true } as any)).toBe(true)
  })

  test('currentKiosk reads native window state when window exists', () => {
    ;(isMacPlatform as jest.Mock).mockReturnValue(false)
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => false),
      isKiosk: jest.fn(() => true)
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
})
