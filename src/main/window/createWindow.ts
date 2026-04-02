import { is } from '@electron-toolkit/utils'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import { BrowserWindow, screen, session, shell } from 'electron'
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

export function createMainWindow(runtimeState: runtimeStateProps, services: ServicesProps) {
  const { projectionService } = services
  const isMac = isMacPlatform()

  mainWindow = new BrowserWindow({
    width: runtimeState.config.width,
    height: runtimeState.config.height,
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

    if (runtimeState.config.kiosk) {
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
    pushSettingsToRenderer(runtimeState, { kiosk: currentKiosk(runtimeState.config) })

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
