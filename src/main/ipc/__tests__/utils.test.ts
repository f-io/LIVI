import { execFile } from 'node:child_process'
import os from 'node:os'
import {
  configEvents,
  getMacDesiredOwner,
  saveSettings,
  sendUpdateEvent,
  sendUpdateProgress
} from '@main/ipc/utils'
import { applyNullDeletes, pushSettingsToRenderer, sizesEqual } from '@main/utils'
import { getMainWindow } from '@main/window/createWindow'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize
} from '@main/window/utils'
import { screen } from 'electron'
import { existsSync, writeFileSync } from 'fs'

jest.mock('fs', () => ({
  existsSync: jest.fn(() => false),
  writeFileSync: jest.fn()
}))

jest.mock('node:child_process', () => ({
  execFile: jest.fn()
}))

jest.mock('node:os', () => ({
  userInfo: jest.fn(() => ({ username: 'fallback-user' }))
}))

jest.mock('electron', () => ({
  screen: {
    getDisplayMatching: jest.fn(() => ({
      workAreaSize: { width: 1920, height: 1080 }
    }))
  }
}))

jest.mock('@main/config/paths', () => ({
  CONFIG_PATH: '/tmp/config.json'
}))

jest.mock('@shared/types', () => ({
  DEFAULT_BINDINGS: { play: 'Space' }
}))

jest.mock('@main/window/createWindow', () => ({
  getMainWindow: jest.fn()
}))

jest.mock('@main/utils', () => ({
  applyNullDeletes: jest.fn(),
  pushSettingsToRenderer: jest.fn(),
  sizesEqual: jest.fn(() => true)
}))

jest.mock('@main/window/utils', () => ({
  applyAspectRatioFullscreen: jest.fn(),
  applyAspectRatioWindowed: jest.fn(),
  applyWindowedContentSize: jest.fn()
}))

const mockedExistsSync = existsSync as jest.Mock
const mockedWriteFileSync = writeFileSync as jest.Mock
const mockedExecFile = execFile as jest.Mock
const mockedUserInfo = os.userInfo as jest.Mock
const mockedGetMainWindow = getMainWindow as jest.Mock
const mockedApplyNullDeletes = applyNullDeletes as jest.Mock
const mockedPushSettingsToRenderer = pushSettingsToRenderer as jest.Mock
const mockedSizesEqual = sizesEqual as jest.Mock
const mockedApplyAspectRatioFullscreen = applyAspectRatioFullscreen as jest.Mock
const mockedApplyAspectRatioWindowed = applyAspectRatioWindowed as jest.Mock
const mockedApplyWindowedContentSize = applyWindowedContentSize as jest.Mock
const mockedGetDisplayMatching = screen.getDisplayMatching as jest.Mock

describe('ipc utils', () => {
  const originalPlatform = process.platform
  const originalUser = process.env.USER
  const originalSudoUser = process.env.SUDO_USER

  beforeEach(() => {
    jest.clearAllMocks()
    jest.restoreAllMocks()
    configEvents.removeAllListeners('changed')
    mockedExistsSync.mockReturnValue(false)
    mockedSizesEqual.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue(null)
    process.env.USER = originalUser
    process.env.SUDO_USER = originalSudoUser
  })

  afterAll(() => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    process.env.USER = originalUser
    process.env.SUDO_USER = originalSudoUser
  })

  test('sendUpdateEvent forwards payload to renderer channel', () => {
    const send = jest.fn()
    mockedGetMainWindow.mockReturnValue({ webContents: { send } })

    sendUpdateEvent({ phase: 'check' } as never)

    expect(send).toHaveBeenCalledWith('update:event', { phase: 'check' })
  })

  test('sendUpdateEvent does nothing when no main window exists', () => {
    mockedGetMainWindow.mockReturnValue(null)

    expect(() => sendUpdateEvent({ phase: 'check' } as never)).not.toThrow()
  })

  test('sendUpdateProgress forwards payload to renderer progress channel', () => {
    const send = jest.fn()
    mockedGetMainWindow.mockReturnValue({ webContents: { send } })

    sendUpdateProgress({ phase: 'download', percent: 25 } as never)

    expect(send).toHaveBeenCalledWith('update:progress', { phase: 'download', percent: 25 })
  })

  test('sendUpdateProgress does nothing when no main window exists', () => {
    mockedGetMainWindow.mockReturnValue(null)

    expect(() => sendUpdateProgress({ phase: 'download', percent: 25 } as never)).not.toThrow()
  })

  test('getMacDesiredOwner throws on non-mac platforms', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    await expect(getMacDesiredOwner('/Applications/Test.app')).rejects.toThrow('macOS only')
  })

  test('getMacDesiredOwner uses stat owner when destination app exists', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    mockedExistsSync.mockReturnValue(true)
    mockedExecFile.mockImplementation((cmd, args, cb) => {
      if (cmd === 'stat') cb(null, 'alice:wheel')
    })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'alice',
      group: 'wheel'
    })
  })

  test('getMacDesiredOwner falls back to stat user with default staff group when group is missing', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    mockedExistsSync.mockReturnValue(true)
    mockedExecFile.mockImplementation((cmd, args, cb) => {
      if (cmd === 'stat') cb(null, 'alice')
    })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'alice',
      group: 'staff'
    })
  })

  test('getMacDesiredOwner falls back to SUDO_USER and admin group when stat fails', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    process.env.SUDO_USER = 'sudo-user'
    mockedExistsSync.mockReturnValue(true)
    mockedExecFile.mockImplementation((cmd, args, cb) => {
      if (cmd === 'stat') cb(new Error('stat failed'))
      if (cmd === 'id') cb(null, 'staff admin wheel')
    })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'sudo-user',
      group: 'admin'
    })
  })

  test('getMacDesiredOwner falls back to USER and staff when id lookup fails', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    process.env.SUDO_USER = ''
    process.env.USER = 'plain-user'
    mockedExistsSync.mockReturnValue(false)
    mockedExecFile.mockImplementation((cmd, args, cb) => {
      if (cmd === 'id') cb(new Error('id failed'))
    })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'plain-user',
      group: 'staff'
    })
  })

  test('getMacDesiredOwner falls back to os.userInfo username when env users are missing', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    process.env.SUDO_USER = ''
    process.env.USER = ''
    mockedExistsSync.mockReturnValue(false)
    mockedExecFile.mockImplementation((cmd, args, cb) => {
      if (cmd === 'id') cb(new Error('id failed'))
    })
    mockedUserInfo.mockReturnValue({ username: 'os-user' })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'os-user',
      group: 'staff'
    })
  })

  test('saveSettings merges config and bindings, writes file and updates runtime state', () => {
    mockedGetMainWindow.mockReturnValue(null)
    mockedSizesEqual.mockReturnValue(true)

    const onChanged = jest.fn()
    configEvents.on('changed', onChanged)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: true, bindings: { prev: 'ArrowLeft' } }
    } as never

    const patch = { height: 600, bindings: { next: 'ArrowRight' }, language: 'de' } as never

    saveSettings(runtimeState, patch)

    expect(mockedApplyNullDeletes).toHaveBeenCalledWith(runtimeState.config, patch)
    expect(mockedWriteFileSync).toHaveBeenCalledWith(
      '/tmp/config.json',
      JSON.stringify(runtimeState.config, null, 2)
    )
    expect(mockedPushSettingsToRenderer).toHaveBeenCalledWith(runtimeState)

    expect(runtimeState.config.height).toBe(600)
    expect(runtimeState.config.language).toBe('de')
    expect(runtimeState.config.bindings).toEqual({
      play: 'Space',
      prev: 'ArrowLeft',
      next: 'ArrowRight'
    })

    expect(onChanged).toHaveBeenCalledWith(
      runtimeState.config,
      expect.objectContaining({
        width: 800,
        height: 480,
        kiosk: true,
        bindings: { prev: 'ArrowLeft' }
      })
    )
  })

  test('saveSettings warns when config write fails', () => {
    const warnSpy = jest.spyOn(console, 'warn').mockImplementation(() => {})
    mockedWriteFileSync.mockImplementation(() => {
      throw new Error('disk full')
    })
    mockedGetMainWindow.mockReturnValue(null)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, bindings: {} }
    } as any

    saveSettings(runtimeState, { language: 'de' } as any)

    expect(warnSpy).toHaveBeenCalledWith('[config] saveSettings failed:', expect.any(Error))
    expect(mockedPushSettingsToRenderer).toHaveBeenCalledWith(runtimeState)
  })

  test('saveSettings updates zoom factor when window exists', () => {
    const setZoomFactor = jest.fn()
    mockedGetMainWindow.mockReturnValue({
      webContents: { setZoomFactor },
      isFullScreen: jest.fn(() => false),
      setFullScreen: jest.fn()
    })
    mockedSizesEqual.mockReturnValue(true)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, bindings: {}, uiZoomPercent: 125 }
    } as any

    saveSettings(runtimeState, {})

    expect(setZoomFactor).toHaveBeenCalledWith(1.25)
  })

  test('saveSettings on mac enters fullscreen kiosk and applies fullscreen sizing when size changed', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      isFullScreen: jest.fn(() => false),
      setFullScreen: jest.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, bindings: {} }
    } as any

    saveSettings(runtimeState, { kiosk: true, width: 1280, height: 720 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1280, 720)
    expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 1280, 720)
    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('saveSettings on mac leaves fullscreen kiosk and reapplies windowed size when size changed', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      isFullScreen: jest.fn(() => true),
      setFullScreen: jest.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: { width: 1280, height: 720, kiosk: true, bindings: {} }
    } as any

    saveSettings(runtimeState, { kiosk: false, width: 800, height: 480 } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(false)
    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 800, 480)
  })

  test('saveSettings on mac updates fullscreen sizing when kiosk is unchanged and size changes', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      isFullScreen: jest.fn(() => true),
      setFullScreen: jest.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: true, bindings: {} }
    } as any

    saveSettings(runtimeState, { width: 1920, height: 1080 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1920, 1080)
    expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 1920, 1080)
    expect(mainWindow.setFullScreen).not.toHaveBeenCalled()
  })

  test('saveSettings on mac updates windowed sizing when kiosk is unchanged and size changes in windowed mode', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      isFullScreen: jest.fn(() => false),
      setFullScreen: jest.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, bindings: {} }
    } as any

    saveSettings(runtimeState, { width: 1024, height: 600 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1024, 600)
    expect(mockedApplyAspectRatioFullscreen).not.toHaveBeenCalled()
  })

  test('saveSettings on linux entering kiosk removes aspect ratio constraints and sizes to work area', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      setKiosk: jest.fn(),
      getBounds: jest.fn(() => ({ x: 0, y: 0, width: 800, height: 480 })),
      setContentSize: jest.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)
    mockedGetDisplayMatching.mockReturnValue({
      workAreaSize: { width: 1600, height: 900 }
    })

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, bindings: {} }
    } as any

    saveSettings(runtimeState, { kiosk: true } as any)

    expect(mockedApplyAspectRatioWindowed).toHaveBeenCalledWith(mainWindow, 0, 0)
    expect(mainWindow.setKiosk).toHaveBeenCalledWith(true)
    expect(mainWindow.setContentSize).toHaveBeenCalledWith(1600, 900)
  })

  test('saveSettings on linux leaving kiosk reapplies windowed size on resize and immediate tick', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const on = jest.fn()
    const removeListener = jest.fn()
    const isDestroyed = jest.fn(() => false)

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      setKiosk: jest.fn(),
      on,
      removeListener,
      isDestroyed
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)

    const immediateSpy = jest.spyOn(global, 'setImmediate').mockImplementation(((
      fn: (...args: unknown[]) => void
    ) => {
      fn()
      return {} as NodeJS.Immediate
    }) as typeof setImmediate)

    const runtimeState = {
      config: { width: 1280, height: 720, kiosk: true, bindings: {} }
    } as any

    saveSettings(runtimeState, { kiosk: false, width: 800, height: 480 } as any)

    expect(mockedApplyAspectRatioWindowed).toHaveBeenCalledWith(mainWindow, 0, 0)
    expect(mainWindow.setKiosk).toHaveBeenCalledWith(false)
    expect(on).toHaveBeenCalledWith('resize', expect.anything())
    expect(immediateSpy).toHaveBeenCalled()

    const resizeHandler = on.mock.calls.find(([name]) => name === 'resize')?.[1]
    expect(resizeHandler).toBeDefined()

    resizeHandler()
    expect(removeListener).toHaveBeenCalledWith('resize', resizeHandler)
    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 800, 480)
  })

  test('saveSettings on linux skips immediate resize apply when window is destroyed', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      setKiosk: jest.fn(),
      on: jest.fn(),
      removeListener: jest.fn(),
      isDestroyed: jest.fn(() => true)
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)

    jest.spyOn(global, 'setImmediate').mockImplementation(((fn: (...args: unknown[]) => void) => {
      fn()
      return {} as NodeJS.Immediate
    }) as typeof setImmediate)

    const runtimeState = {
      config: { width: 1280, height: 720, kiosk: true, bindings: {} }
    } as any

    saveSettings(runtimeState, { kiosk: false, width: 800, height: 480 } as any)

    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
  })

  test('saveSettings on linux applies windowed size when only size changes outside kiosk', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: jest.fn() },
      setKiosk: jest.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: { width: 800, height: 480, kiosk: false, bindings: {} }
    } as any

    saveSettings(runtimeState, { width: 1024, height: 600 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1024, 600)
  })
})
