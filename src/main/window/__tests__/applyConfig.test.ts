import { sizesEqual } from '@main/utils'
import { applyConfig, configEvents } from '@main/window/applyConfig'
import { getMainWindow } from '@main/window/createWindow'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize
} from '@main/window/utils'
import { screen } from 'electron'
import type { Mock } from 'vitest'

vi.mock('electron', () => ({
  screen: {
    getDisplayMatching: vi.fn(function () {
      return {
        workAreaSize: { width: 1920, height: 1080 }
      }
    })
  }
}))

vi.mock('@main/window/createWindow', () => ({
  getMainWindow: vi.fn()
}))

vi.mock('@main/utils', () => ({
  sizesEqual: vi.fn(() => true)
}))

vi.mock('@main/window/utils', () => ({
  applyAspectRatioFullscreen: vi.fn(),
  applyAspectRatioWindowed: vi.fn(),
  applyWindowedContentSize: vi.fn(),
  uiZoomFactor: vi.fn((pct?: number) => (pct ?? 100) / 100)
}))

const mockedGetMainWindow = getMainWindow as Mock
const mockedSizesEqual = sizesEqual as Mock
const mockedApplyAspectRatioFullscreen = applyAspectRatioFullscreen as Mock
const mockedApplyAspectRatioWindowed = applyAspectRatioWindowed as Mock
const mockedApplyWindowedContentSize = applyWindowedContentSize as Mock
const mockedGetDisplayMatching = screen.getDisplayMatching as Mock

// Core sends the whole config, the tests describe it as a change to the last one.
const applyChange = (runtimeState: any, patch: any) =>
  applyConfig(runtimeState, { ...runtimeState.config, ...patch })

describe('applyConfig', () => {
  const originalPlatform = process.platform

  beforeEach(async () => {
    vi.clearAllMocks()
    vi.restoreAllMocks()
    configEvents.removeAllListeners('changed')
    mockedSizesEqual.mockReturnValue(true)
    mockedGetMainWindow.mockReturnValue(null)
  })

  afterAll(async () => {
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  test('takes core config as it is and tells the listeners', async () => {
    mockedGetMainWindow.mockReturnValue(null)

    const onChanged = vi.fn()
    configEvents.on('changed', onChanged)

    const prev = { mainScreenWidth: 800, mainScreenHeight: 480 }
    const next = { mainScreenWidth: 800, mainScreenHeight: 600, language: 'de' }
    const runtimeState = { config: prev } as never

    applyConfig(runtimeState, next as never)

    expect((runtimeState as { config: unknown }).config).toBe(next)
    expect(onChanged).toHaveBeenCalledWith(next, prev)
  })

  test('applyConfig updates zoom factor when window exists', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {},
        uiZoomPercent: 125
      }
    } as any

    applyChange(runtimeState, {})

    expect(setZoomFactor).toHaveBeenCalledWith(1.25)
  })

  test('applyConfig on mac enters fullscreen kiosk and applies fullscreen sizing when size changed', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, {
      kiosk: { main: true, cluster: false, aux: false },
      mainScreenWidth: 1280,
      mainScreenHeight: 720
    } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1280, 720)
    expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 1280, 720)
    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
  })

  test('applyConfig on mac leaves fullscreen kiosk and reapplies windowed size when size changed', async () => {
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
        kiosk: { main: true, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, {
      kiosk: { main: false, cluster: false, aux: false },
      mainScreenWidth: 800,
      mainScreenHeight: 480
    } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(false)
    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 800, 480)
  })

  test('applyConfig on mac updates fullscreen sizing when kiosk is unchanged and size changes', async () => {
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
        kiosk: { main: true, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { mainScreenWidth: 1920, mainScreenHeight: 1080 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1920, 1080)
    expect(mockedApplyAspectRatioFullscreen).toHaveBeenCalledWith(mainWindow, 1920, 1080)
    expect(mainWindow.setFullScreen).not.toHaveBeenCalled()
  })

  test('applyConfig on mac updates windowed sizing when kiosk is unchanged and size changes in windowed mode', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { mainScreenWidth: 1024, mainScreenHeight: 600 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1024, 600)
    expect(mockedApplyAspectRatioFullscreen).not.toHaveBeenCalled()
  })

  test('applyConfig on linux entering kiosk removes aspect ratio constraints and sizes to work area', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { kiosk: { main: true, cluster: false, aux: false } } as any)

    expect(mockedApplyAspectRatioWindowed).toHaveBeenCalledWith(mainWindow, 0, 0)
    expect(mainWindow.setKiosk).toHaveBeenCalledWith(true)
    expect(mainWindow.setContentSize).toHaveBeenCalledWith(1600, 900)
  })

  test('applyConfig on linux leaving kiosk reapplies windowed size on resize and immediate tick', async () => {
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
        kiosk: { main: true, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, {
      kiosk: { main: false, cluster: false, aux: false },
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

  test('applyConfig on linux skips immediate resize apply when window is destroyed', async () => {
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
        kiosk: { main: true, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, {
      kiosk: { main: false, cluster: false, aux: false },
      mainScreenWidth: 800,
      mainScreenHeight: 480
    } as any)

    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
  })

  test('applyConfig on linux applies windowed size when only size changes outside kiosk', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { mainScreenWidth: 1024, mainScreenHeight: 600 } as any)

    expect(mockedApplyWindowedContentSize).toHaveBeenCalledWith(mainWindow, 1024, 600)
  })

  test('applyConfig on mac enters fullscreen kiosk without size change', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { kiosk: { main: true, cluster: false, aux: false } } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
    expect(mockedApplyAspectRatioFullscreen).not.toHaveBeenCalled()
  })

  test('applyConfig on mac leaves fullscreen kiosk without size change', async () => {
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
        kiosk: { main: true, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { kiosk: { main: false, cluster: false, aux: false } } as any)

    expect(mainWindow.setFullScreen).toHaveBeenCalledWith(false)
    expect(mockedApplyWindowedContentSize).not.toHaveBeenCalled()
  })

  test('applyConfig on linux does not apply windowed size when size changes in kiosk mode', async () => {
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
        kiosk: { main: true, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    applyChange(runtimeState, { mainScreenWidth: 1024, mainScreenHeight: 600 } as any)

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
    'applyConfig on mac falls back to 800x480 when sizes are unset (%s)',
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
          kiosk: { main: prevKiosk, cluster: false, aux: false }
        }
      } as any

      applyChange(runtimeState, { kiosk: { main: nextKiosk, cluster: false, aux: false } } as any)

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

  test('applyConfig on linux under the compositor only toggles fullscreen', async () => {
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
        kiosk: { main: false, cluster: false, aux: false },
        bindings: {}
      }
    } as any

    try {
      applyChange(runtimeState, { kiosk: { main: true, cluster: false, aux: false } } as any)
      expect(mainWindow.setFullScreen).toHaveBeenCalledWith(true)
      expect(mainWindow.setKiosk).not.toHaveBeenCalled()

      mainWindow.setFullScreen.mockClear()
      applyChange(runtimeState, { mainScreenWidth: 1024 } as any)
      expect(mainWindow.setFullScreen).not.toHaveBeenCalled()
    } finally {
      delete process.env.LIVI_COMPOSITOR
    }
  })
})
