import { is } from '@electron-toolkit/utils'
import { saveSettings } from '@main/ipc/utils'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import type { WindowBounds } from '@shared/types'
import { app, BrowserWindow, screen, session, shell } from 'electron'
import { join } from 'path'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  attachKioskStateSync,
  currentKiosk,
  persistKioskAndBroadcast
} from './utils'

let mainWindow: BrowserWindow | null = null

function readMainBounds(rs: runtimeStateProps): WindowBounds | undefined {
  const b = rs.config.mainScreenBounds
  if (
    b &&
    typeof b === 'object' &&
    typeof b.x === 'number' &&
    typeof b.y === 'number' &&
    typeof b.width === 'number' &&
    typeof b.height === 'number'
  ) {
    return b
  }
  return undefined
}

export function createMainWindow(runtimeState: runtimeStateProps, services: ServicesProps) {
  const { projectionService } = services
  const isMac = isMacPlatform()

  const savedBounds = readMainBounds(runtimeState)

  mainWindow = new BrowserWindow({
    width: savedBounds?.width ?? runtimeState.config.width,
    height: savedBounds?.height ?? runtimeState.config.height,
    x: savedBounds?.x,
    y: savedBounds?.y,
    frame: true,
    useContentSize: true,
    kiosk: false,
    autoHideMenuBar: true,
    backgroundColor: '#000',
    fullscreenable: true,
    simpleFullscreen: false,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: false,
      nodeIntegration: false,
      contextIsolation: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      backgroundThrottling: false,
      experimentalFeatures: true
    }
  })

  // Re-apply bounds after the compositor shows the window
  if (savedBounds) {
    mainWindow.once('ready-to-show', () => {
      if (!mainWindow || mainWindow.isDestroyed()) return
      mainWindow.setBounds({
        x: savedBounds.x,
        y: savedBounds.y,
        width: savedBounds.width,
        height: savedBounds.height
      })
    })
  }

  // Persist last-known geometry on move/resize
  let boundsTimer: NodeJS.Timeout | null = null
  const persistMainBounds = () => {
    if (!mainWindow || mainWindow.isDestroyed()) return
    try {
      if (mainWindow.isFullScreen()) return
      if (typeof mainWindow.isKiosk === 'function' && mainWindow.isKiosk()) return
    } catch {
      // mock / shim
    }
    if (typeof mainWindow.getPosition !== 'function') return
    if (typeof mainWindow.getContentSize !== 'function') return
    const [x, y] = mainWindow.getPosition()
    const [width, height] = mainWindow.getContentSize()
    const next: WindowBounds = { x, y, width, height }
    const prev = runtimeState.config.mainScreenBounds
    if (
      prev &&
      prev.x === next.x &&
      prev.y === next.y &&
      prev.width === next.width &&
      prev.height === next.height
    ) {
      return
    }
    saveSettings(runtimeState, { mainScreenBounds: next })
  }
  const scheduleMainBoundsSave = () => {
    if (boundsTimer) clearTimeout(boundsTimer)
    boundsTimer = setTimeout(() => {
      boundsTimer = null
      persistMainBounds()
    }, 500)
  }
  mainWindow.on('move', scheduleMainBoundsSave)
  mainWindow.on('moved', scheduleMainBoundsSave)
  mainWindow.on('resize', scheduleMainBoundsSave)
  mainWindow.on('resized', scheduleMainBoundsSave)

  // keep in sync with WM
  attachKioskStateSync(runtimeState)

  const ses = mainWindow.webContents.session
  ses.setPermissionCheckHandler((_w, p) => ['usb', 'hid', 'media', 'display-capture'].includes(p))
  ses.setPermissionRequestHandler((_w, p, cb) =>
    cb(['usb', 'hid', 'media', 'display-capture'].includes(p))
  )
  ses.setUSBProtectedClassesHandler(({ protectedClasses }) =>
    protectedClasses.filter((c) => ['audio', 'video', 'vendor-specific'].includes(c))
  )

  session.defaultSession.webRequest.onHeadersReceived(
    { urls: ['*://*/*', 'file://*/*'] },
    (d, cb) =>
      cb({
        responseHeaders: {
          ...d.responseHeaders,
          'Cross-Origin-Opener-Policy': ['same-origin'],
          'Cross-Origin-Embedder-Policy': ['require-corp'],
          'Cross-Origin-Resource-Policy': ['same-site']
        }
      })
  )

  mainWindow.once('ready-to-show', () => {
    if (!mainWindow) return

    const baseW = runtimeState.config.width || 800
    const baseH = runtimeState.config.height || 480

    // always start windowed
    applyWindowedContentSize(mainWindow, baseW, baseH)
    mainWindow.show()

    // Snapshot the geometry
    scheduleMainBoundsSave()

    if (runtimeState.config.kiosk?.main) {
      setImmediate(() => {
        if (!mainWindow || mainWindow.isDestroyed()) return

        if (isMac) {
          mainWindow.setFullScreen(true)
        } else {
          mainWindow.setKiosk(true)

          const d = screen.getDisplayMatching(mainWindow.getBounds())
          const wa = d.workAreaSize

          mainWindow.setContentSize(wa.width, wa.height)
        }
      })
    }

    mainWindow.webContents.setZoomFactor((runtimeState.config.uiZoomPercent ?? 100) / 100)
    pushSettingsToRenderer(runtimeState, {
      kiosk: { ...runtimeState.config.kiosk, main: currentKiosk(runtimeState.config) }
    })

    if (is.dev) mainWindow.webContents.openDevTools({ mode: 'detach' })
    projectionService.attachRenderer(mainWindow.webContents)
  })

  if (isMac) {
    mainWindow.on('enter-full-screen', () => {
      if (runtimeState.suppressNextFsSync) return
      applyAspectRatioFullscreen(
        mainWindow!,
        runtimeState.config.width || 800,
        runtimeState.config.height || 480
      )
      persistKioskAndBroadcast(true, runtimeState)
    })

    mainWindow.on('leave-full-screen', () => {
      if (runtimeState.suppressNextFsSync) {
        runtimeState.suppressNextFsSync = false
        return
      }
      applyAspectRatioWindowed(
        mainWindow!,
        runtimeState.config.width || 800,
        runtimeState.config.height || 480
      )
      persistKioskAndBroadcast(false, runtimeState)
    })
  }

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  if (is.dev && process.env.ELECTRON_RENDERER_URL) {
    mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  } else mainWindow.loadURL('app://index.html')

  mainWindow.on('close', (e) => {
    if (isMac && !runtimeState.isQuitting) {
      e.preventDefault()
      if (mainWindow!.isFullScreen()) {
        runtimeState.suppressNextFsSync = true
        mainWindow!.once('leave-full-screen', () => mainWindow?.hide())
        mainWindow!.setFullScreen(false)
      } else {
        mainWindow!.hide()
      }
      return
    }

    if (!runtimeState.isQuitting) {
      e.preventDefault()
      app.quit()
    }
  })

  if (is.dev) {
    const gpuWindow = new BrowserWindow({
      width: 1000,
      height: 800,
      title: 'GPU Info',
      webPreferences: { nodeIntegration: false, contextIsolation: true }
    })
    gpuWindow.loadURL('chrome://gpu')
  }

  if (is.dev) {
    const mediaWindow = new BrowserWindow({
      width: 1000,
      height: 800,
      title: 'GPU Info',
      webPreferences: { nodeIntegration: false, contextIsolation: true }
    })
    mediaWindow.loadURL('chrome://media-internals')
  }
}

export function getMainWindow() {
  return mainWindow
}
