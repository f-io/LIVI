import { app } from 'electron'
import {
  applyNullDeletes,
  isMacPlatform,
  linuxPresetAngleVulkan,
  pushSettingsToRenderer,
  setFeatureFlags,
  sizesEqual
} from '@main/utils'
import { getMainWindow } from '@main/window/createWindow'

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: jest.fn()
}))

describe('main utils', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('isMacPlatform reflects current process platform', () => {
    const original = process.platform
    try {
      Object.defineProperty(process, 'platform', { value: 'darwin' })
      expect(isMacPlatform()).toBe(true)
    } finally {
      Object.defineProperty(process, 'platform', { value: original })
    }
  })

  test('isMacPlatform is false on non-mac platforms', () => {
    const original = process.platform
    try {
      Object.defineProperty(process, 'platform', { value: 'linux' })
      expect(isMacPlatform()).toBe(false)
    } finally {
      Object.defineProperty(process, 'platform', { value: original })
    }
  })

  test('applyNullDeletes removes explicitly-null reset fields', () => {
    const merged = {
      primaryColorDark: '#111',
      highlightColorDark: '#222',
      keep: 'yes'
    } as any

    applyNullDeletes(merged, {
      primaryColorDark: null,
      highlightColorDark: null,
      keep: 'no'
    } as any)

    expect(merged.primaryColorDark).toBeUndefined()
    expect(merged.highlightColorDark).toBeUndefined()
    expect(merged.keep).toBe('yes')
  })

  test('applyNullDeletes only removes null values for configured reset fields', () => {
    const merged = {
      primaryColorDark: '#111',
      highlightColorDark: '#222',
      keep: 'yes'
    } as any

    applyNullDeletes(merged, {
      primaryColorDark: undefined,
      highlightColorDark: '#333',
      keep: null
    } as any)

    expect(merged.primaryColorDark).toBe('#111')
    expect(merged.highlightColorDark).toBe('#222')
    expect(merged.keep).toBe('yes')
  })

  test('sizesEqual compares normalized width and height', () => {
    expect(
      sizesEqual({ width: 800, height: 480 } as any, { width: '800', height: 480 } as any)
    ).toBe(true)
    expect(sizesEqual({ width: 800, height: 480 } as any, { width: 801, height: 480 } as any)).toBe(
      false
    )
  })

  test('sizesEqual treats missing or invalid dimensions as zero', () => {
    expect(sizesEqual({ width: undefined, height: undefined } as any, {} as any)).toBe(true)
    expect(sizesEqual({ width: 'abc', height: null } as any, { width: 0, height: 0 } as any)).toBe(
      true
    )
    expect(sizesEqual({ width: '', height: 5 } as any, { width: 0, height: 0 } as any)).toBe(false)
  })

  test('setFeatureFlags emits comma-joined enable-features switch', () => {
    setFeatureFlags(['A', 'B'])
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('enable-features', 'A,B')
  })

  test('linuxPresetAngleVulkan emits gpu switches and feature flags', () => {
    linuxPresetAngleVulkan()

    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('use-gl', 'angle')
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('use-angle', 'vulkan')
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith('ozone-platform-hint', 'auto')
    expect(app.commandLine.appendSwitch).toHaveBeenCalledWith(
      'enable-features',
      expect.stringContaining('Vulkan')
    )
  })

  test('pushSettingsToRenderer sends settings when main window is alive', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => false),
      webContents: { send }
    })

    const runtimeState = { config: { kiosk: true, language: 'en' } } as any
    pushSettingsToRenderer(runtimeState, { kiosk: false })

    expect(send).toHaveBeenCalledWith('settings', { kiosk: false, language: 'en' })
  })

  test('pushSettingsToRenderer sends runtime config unchanged when no override is given', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => false),
      webContents: { send }
    })

    pushSettingsToRenderer({ config: { kiosk: true, language: 'de' } } as any)

    expect(send).toHaveBeenCalledWith('settings', { kiosk: true, language: 'de' })
  })

  test('pushSettingsToRenderer does nothing when no window', () => {
    ;(getMainWindow as jest.Mock).mockReturnValue(null)

    pushSettingsToRenderer({ config: { kiosk: true } } as any)

    expect(getMainWindow).toHaveBeenCalled()
  })

  test('pushSettingsToRenderer does nothing when window is destroyed', () => {
    const send = jest.fn()
    ;(getMainWindow as jest.Mock).mockReturnValue({
      isDestroyed: jest.fn(() => true),
      webContents: { send }
    })

    pushSettingsToRenderer({ config: { kiosk: true } } as any)

    expect(send).not.toHaveBeenCalled()
  })
})
