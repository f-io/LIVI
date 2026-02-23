import { isMacPlatform } from '@main/utils'
import { BrowserWindow, session, shell } from 'electron'
import { is } from '@electron-toolkit/utils'
import { join } from 'path'
import { CarplayService } from '@main/services/carplay/services/CarplayService'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  currentKiosk,
  persistKioskAndBroadcast,
  sendKioskSync
} from './utils'
import { runtimeStateProps } from '@main/types'
import { DEFAULT_HEIGHT, DEFAULT_WIDTH } from '@main/constants'

let mainWindow: BrowserWindow | null = null

const carplayService = new CarplayService()

declare global {
  var carplayService: CarplayService | undefined
}
globalThis.carplayService = carplayService

export function createMainWindow(runtimeState: runtimeStateProps) {
  const { isQuitting, suppressNextFsSync } = runtimeState
  const isMac = isMacPlatform()

  mainWindow = new BrowserWindow({
    width: runtimeState.config.width,
    height: runtimeState.config.height,
    frame: isMac ? true : !runtimeState.config.kiosk,
    useContentSize: true,
    kiosk: isMac ? false : runtimeState.config.kiosk,
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
      experimentalFeatures: true
    }
  })

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

    if (isMac) {
      const baseW = runtimeState.config.width || DEFAULT_WIDTH
      const baseH = runtimeState.config.height || DEFAULT_HEIGHT
      applyWindowedContentSize(mainWindow, baseW, baseH)
      mainWindow.show()
      if (runtimeState.config.kiosk) setImmediate(() => mainWindow!.setFullScreen(true))
    } else {
      if (runtimeState.config.kiosk) {
        mainWindow.setKiosk(true)
        applyAspectRatioWindowed(mainWindow, 0, 0)
      } else {
        mainWindow.setContentSize(runtimeState.config.width, runtimeState.config.height, false)
        applyAspectRatioWindowed(mainWindow, runtimeState.config.width, runtimeState.config.height)
      }
      mainWindow.show()
    }

    sendKioskSync(currentKiosk(runtimeState.config))

    if (is.dev) mainWindow.webContents.openDevTools({ mode: 'detach' })
    carplayService.attachRenderer(mainWindow.webContents)
  })

  if (isMac) {
    mainWindow.on('enter-full-screen', () => {
      if (suppressNextFsSync) return
      applyAspectRatioFullscreen(
        mainWindow!,
        runtimeState.config.width || 800,
        runtimeState.config.height || 480
      )
      persistKioskAndBroadcast(true, runtimeState)
    })

    mainWindow.on('leave-full-screen', () => {
      if (suppressNextFsSync) {
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

  if (is.dev && process.env.ELECTRON_RENDERER_URL)
    mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
  else mainWindow.loadURL('app://index.html')

  mainWindow.on('close', (e) => {
    if (isMac && !isQuitting) {
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
