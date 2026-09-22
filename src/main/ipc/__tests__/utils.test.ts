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
import type { Mock } from 'vitest'

const fsMock = vi.hoisted(() => ({
  existsSync: vi.fn(() => false),
  readFileSync: vi.fn(),
  renameSync: vi.fn(),
  writeFileSync: vi.fn()
}))

vi.mock('fs', () => ({ ...fsMock, default: fsMock }))
vi.mock('node:fs', () => ({ ...fsMock, default: fsMock }))

vi.mock('node:child_process', () => ({
  execFile: vi.fn()
}))

vi.mock('node:os', () => {
  const __m = {
    userInfo: vi.fn(function () {
      return { username: 'fallback-user' }
    })
  }
  return { ...__m, default: __m }
})

vi.mock('electron', () => ({
  screen: {
    getDisplayMatching: vi.fn(function () {
      return {
        workAreaSize: { width: 1920, height: 1080 }
      }
    })
  }
}))

vi.mock('@main/config/paths', () => ({
  CONFIG_PATH: '/tmp/config.json'
}))

vi.mock('@shared/types', () => ({
  DEFAULT_BINDINGS: { play: 'Space' }
}))

vi.mock('@main/window/createWindow', () => ({
  getMainWindow: vi.fn()
}))

vi.mock('@main/utils', () => ({
  applyNullDeletes: vi.fn(),
  pushSettingsToRenderer: vi.fn(),
  sizesEqual: vi.fn(() => true)
}))

vi.mock('@main/window/utils', () => ({
  applyAspectRatioFullscreen: vi.fn(),
  applyAspectRatioWindowed: vi.fn(),
  applyWindowedContentSize: vi.fn(),
  uiZoomFactor: vi.fn((pct?: number) => (pct ?? 100) / 100)
}))

const mockedExistsSync = existsSync as Mock
const mockedWriteFileSync = writeFileSync as Mock
const mockedExecFile = execFile as Mock
const mockedUserInfo = os.userInfo as Mock
const mockedGetMainWindow = getMainWindow as Mock
const mockedApplyNullDeletes = applyNullDeletes as Mock
const mockedPushSettingsToRenderer = pushSettingsToRenderer as Mock
const mockedSizesEqual = sizesEqual as Mock
const mockedApplyAspectRatioFullscreen = applyAspectRatioFullscreen as Mock
const mockedApplyAspectRatioWindowed = applyAspectRatioWindowed as Mock
const mockedApplyWindowedContentSize = applyWindowedContentSize as Mock
const mockedGetDisplayMatching = screen.getDisplayMatching as Mock

describe('ipc utils', () => {
  const originalPlatform = process.platform
  const originalUser = process.env.USER
  const originalSudoUser = process.env.SUDO_USER

  beforeEach(async () => {
    vi.clearAllMocks()
    vi.restoreAllMocks()
    configEvents.removeAllListeners('changed')
    mockedExistsSync.mockReturnValue(false)
    mockedSizesEqual.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue(null)
    process.env.USER = originalUser
    process.env.SUDO_USER = originalSudoUser
  })

  afterAll(async () => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
    process.env.USER = originalUser
    process.env.SUDO_USER = originalSudoUser
  })

  test('sendUpdateEvent forwards payload to renderer channel', async () => {
    const send = vi.fn()
    mockedGetMainWindow.mockReturnValue({ webContents: { send } })

    sendUpdateEvent({ phase: 'check' } as never)

    expect(send).toHaveBeenCalledWith('update:event', { phase: 'check' })
  })

  test('sendUpdateEvent does nothing when no main window exists', async () => {
    mockedGetMainWindow.mockReturnValue(null)

    expect(() => sendUpdateEvent({ phase: 'check' } as never)).not.toThrow()
  })

  test('sendUpdateProgress forwards payload to renderer progress channel', async () => {
    const send = vi.fn()
    mockedGetMainWindow.mockReturnValue({ webContents: { send } })

    sendUpdateProgress({ phase: 'download', percent: 25 } as never)

    expect(send).toHaveBeenCalledWith('update:progress', { phase: 'download', percent: 25 })
  })

  test('sendUpdateProgress does nothing when no main window exists', async () => {
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
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
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
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
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
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
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
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
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
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
      if (cmd === 'id') cb(new Error('id failed'))
    })
    mockedUserInfo.mockReturnValue({ username: 'os-user' })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'os-user',
      group: 'staff'
    })
  })

  test('getMacDesiredOwner falls back when stat returns empty user portion', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    process.env.SUDO_USER = ''
    process.env.USER = 'fallback-user'
    mockedExistsSync.mockReturnValue(true)
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
      // stat returns a colon-only string — user part is empty, so if (user) is false
      if (cmd === 'stat') cb(null, ':wheel')
      if (cmd === 'id') cb(null, 'staff wheel')
    })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'fallback-user',
      group: 'staff'
    })
  })

  test('getMacDesiredOwner uses staff group when id succeeds but admin is not listed', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })
    process.env.SUDO_USER = 'sudo-user'
    mockedExistsSync.mockReturnValue(true)
    mockedExecFile.mockImplementation(function (cmd, args, cb) {
      if (cmd === 'stat') cb(new Error('stat failed'))
      // id returns groups without 'admin'
      if (cmd === 'id') cb(null, 'staff wheel dialout')
    })

    await expect(getMacDesiredOwner('/Applications/Test.app')).resolves.toEqual({
      user: 'sudo-user',
      group: 'staff'
    })
  })

  test('saveSettings merges config and bindings, writes file and updates runtime state', async () => {
    mockedGetMainWindow.mockReturnValue(null)
    mockedSizesEqual.mockReturnValue(true)

    const onChanged = vi.fn()
    configEvents.on('changed', onChanged)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        bindings: { prev: 'ArrowLeft' }
      }
    } as never

    const patch = {
      mainScreenHeight: 600,
      bindings: { next: 'ArrowRight' },
      language: 'de'
    } as never

    saveSettings(runtimeState, patch)

    expect(mockedApplyNullDeletes).toHaveBeenCalledWith(runtimeState.config, patch)
    expect(mockedWriteFileSync).toHaveBeenCalledWith(
      '/tmp/config.json.tmp',
      JSON.stringify(runtimeState.config, null, 2)
    )
    expect(fsMock.renameSync).toHaveBeenCalledWith('/tmp/config.json.tmp', '/tmp/config.json')
    expect(mockedPushSettingsToRenderer).toHaveBeenCalledWith(runtimeState)

    expect(runtimeState.config.mainScreenHeight).toBe(600)
    expect(runtimeState.config.language).toBe('de')
    expect(runtimeState.config.bindings).toEqual({
      play: 'Space',
      prev: 'ArrowLeft',
      next: 'ArrowRight'
    })

    expect(onChanged).toHaveBeenCalledWith(
      runtimeState.config,
      expect.objectContaining({
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        bindings: { prev: 'ArrowLeft' }
      })
    )
  })

  test('saveSettings warns when config write fails', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(function () {})
    mockedWriteFileSync.mockImplementation(function () {
      throw new Error('disk full')
    })
    mockedGetMainWindow.mockReturnValue(null)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { language: 'de' } as any)

    expect(warnSpy).toHaveBeenCalledWith('[config] saveSettings failed:', expect.any(Error))
    expect(mockedPushSettingsToRenderer).toHaveBeenCalledWith(runtimeState)
  })

  test('saveSettings updates zoom factor when window exists', async () => {
    const setZoomFactor = vi.fn()
    mockedGetMainWindow.mockReturnValue({
      webContents: { setZoomFactor },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => false),
      setFullScreen: vi.fn()
    })
    mockedSizesEqual.mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {},
        uiZoomPercent: 125
      }
    } as any

    saveSettings(runtimeState, {})

    expect(setZoomFactor).toHaveBeenCalledWith(1.25)
  })

  test('saveSettings on mac enters fullscreen kiosk and applies fullscreen sizing when size changed', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => false),
      setFullScreen: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, {
      kiosk: { main: true, dash: false, aux: false },
      mainScreenWidth: 1280,
      mainScreenHeight: 720
    } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1280, 720)
    expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 1280, 720)
    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('saveSettings on mac leaves fullscreen kiosk and reapplies windowed size when size changed', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => true),
      setFullScreen: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: {
        mainScreenWidth: 1280,
        mainScreenHeight: 720,
        kiosk: { main: true, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, {
      kiosk: { main: false, dash: false, aux: false },
      mainScreenWidth: 800,
      mainScreenHeight: 480
    } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(false)
    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 800, 480)
  })

  test('saveSettings on mac updates fullscreen sizing when kiosk is unchanged and size changes', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => true),
      setFullScreen: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { mainScreenWidth: 1920, mainScreenHeight: 1080 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1920, 1080)
    expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 1920, 1080)
    expect(mainWindow.setFullScreen).not.toHaveBeenCalled()
  })

  test('saveSettings on mac updates windowed sizing when kiosk is unchanged and size changes in windowed mode', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => false),
      setFullScreen: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { mainScreenWidth: 1024, mainScreenHeight: 600 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1024, 600)
    expect(mockedApplyAspectRatioFullscreen).not.toHaveBeenCalled()
  })

  test('saveSettings on linux entering kiosk removes aspect ratio constraints and sizes to work area', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      setKiosk: vi.fn(),
      getBounds: vi.fn(function () {
        return { x: 0, y: 0, width: 800, height: 480 }
      }),
      setContentSize: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)
    mockedGetDisplayMatching.mockReturnValue({
      workAreaSize: { width: 1600, height: 900 }
    })

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { kiosk: { main: true, dash: false, aux: false } } as any)

    expect(mockedApplyAspectRatioWindowed).toHaveBeenCalledWith(mainWindow, 0, 0)
    expect(mainWindow.setKiosk).toHaveBeenCalledWith(true)
    expect(mainWindow.setContentSize).toHaveBeenCalledWith(1600, 900)
  })

  test('saveSettings on linux leaving kiosk reapplies windowed size on resize and immediate tick', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const on = vi.fn()
    const removeListener = vi.fn()
    const isDestroyed = vi.fn(() => false)

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      setKiosk: vi.fn(),
      on,
      removeListener,
      isDestroyed
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)

    const immediateSpy = vi.spyOn(global, 'setImmediate').mockImplementation(((
      fn: (...args: unknown[]) => void
    ) => {
      fn()
      return {} as NodeJS.Immediate
    }) as typeof setImmediate)
    const runtimeState = {
      config: {
        mainScreenWidth: 1280,
        mainScreenHeight: 720,
        kiosk: { main: true, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, {
      kiosk: { main: false, dash: false, aux: false },
      mainScreenWidth: 800,
      mainScreenHeight: 480
    } as any)

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

  test('saveSettings on linux skips immediate resize apply when window is destroyed', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      setKiosk: vi.fn(),
      on: vi.fn(),
      removeListener: vi.fn(),
      isDestroyed: vi.fn(() => true)
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)

    vi.spyOn(global, 'setImmediate').mockImplementation(((fn: (...args: unknown[]) => void) => {
      fn()
      return {} as NodeJS.Immediate
    }) as typeof setImmediate)
    const runtimeState = {
      config: {
        mainScreenWidth: 1280,
        mainScreenHeight: 720,
        kiosk: { main: true, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, {
      kiosk: { main: false, dash: false, aux: false },
      mainScreenWidth: 800,
      mainScreenHeight: 480
    } as any)

    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
  })

  test('saveSettings on linux applies windowed size when only size changes outside kiosk', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      setKiosk: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { mainScreenWidth: 1024, mainScreenHeight: 600 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1024, 600)
  })

  test('saveSettings on mac enters fullscreen kiosk without size change', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => false),
      setFullScreen: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { kiosk: { main: true, dash: false, aux: false } } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
    expect(mockedApplyAspectRatioFullscreen).not.toHaveBeenCalled()
  })

  test('saveSettings on mac leaves fullscreen kiosk without size change', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      isFullScreen: vi.fn(() => true),
      setFullScreen: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(true)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { kiosk: { main: false, dash: false, aux: false } } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(false)
    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
  })

  test('saveSettings on linux does not apply windowed size when size changes in kiosk mode', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      setKiosk: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)
    mockedSizesEqual.mockReturnValue(false)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: true, dash: false, aux: false },
        bindings: {}
      }
    } as any

    saveSettings(runtimeState, { mainScreenWidth: 1024, mainScreenHeight: 600 } as any)

    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
  })

  test.each([
    ['entering fullscreen kiosk', true, false, false, true],
    ['entering kiosk while already fullscreen', true, false, true, false],
    ['leaving fullscreen kiosk', false, true, true, true],
    ['leaving kiosk while already windowed', false, true, false, false],
    ['fullscreen without kiosk change', true, true, true, false],
    ['windowed without kiosk change', false, false, false, false]
  ])(
    'saveSettings on mac falls back to 800x480 when sizes are unset (%s)',
    async (_label, nextKiosk, prevKiosk, isFs, expectToggle) => {
      Object.defineProperty(process, 'platform', { value: 'darwin' })

      const mainWindow = {
        webContents: { setZoomFactor: vi.fn() },
        getContentSize: vi.fn(() => [1280, 720]),
        isFullScreen: vi.fn(() => isFs),
        setFullScreen: vi.fn()
      }
      mockedGetMainWindow.mockReturnValue(mainWindow)
      mockedSizesEqual.mockReturnValue(false)

      const runtimeState = {
        config: {
          kiosk: { main: prevKiosk, dash: false, aux: false }
        }
      } as any

      saveSettings(runtimeState, { kiosk: { main: nextKiosk, dash: false, aux: false } } as any)

      expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 800, 480)
      if (nextKiosk && nextKiosk !== prevKiosk) {
        expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 800, 480)
      }
      if (expectToggle) {
        expect(mainWindow.setFullScreen).toHaveBeenCalledWith(nextKiosk)
      } else {
        expect(mainWindow.setFullScreen).not.toHaveBeenCalled()
      }
    }
  )

  test('saveSettings on linux under the compositor only toggles fullscreen', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })
    process.env.LIVI_COMPOSITOR = '1'

    const mainWindow = {
      webContents: { setZoomFactor: vi.fn() },
      getContentSize: vi.fn(() => [1280, 720]),
      setFullScreen: vi.fn(),
      setKiosk: vi.fn()
    }
    mockedGetMainWindow.mockReturnValue(mainWindow)

    const runtimeState = {
      config: {
        mainScreenWidth: 800,
        mainScreenHeight: 480,
        kiosk: { main: false, dash: false, aux: false },
        bindings: {}
      }
    } as any

    try {
      saveSettings(runtimeState, { kiosk: { main: true, dash: false, aux: false } } as any)
      expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
      expect(mainWindow.setKiosk).not.toHaveBeenCalled()

      mainWindow.setFullScreen.mockClear()
      saveSettings(runtimeState, { mainScreenWidth: 1024 } as any)
      expect(mainWindow.setFullScreen).not.toHaveBeenCalled()
    } finally {
      delete process.env.LIVI_COMPOSITOR
    }
  })
})
