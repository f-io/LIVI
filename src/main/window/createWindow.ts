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
  attachResizeReflow,
  currentKiosk,
  persistKioskAndBroadcast,
  sanitizeBounds
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
  const isWin = process.platform === 'win32'
  const compositorMode = process.env.LIVI_COMPOSITOR === '1'
  const wantKiosk = runtimeState.config.kiosk?.main === true || process.env.LIVI_KIOSK === '1'
  const transparentWindow = compositorMode || isMac || isWin
  const winKioskBounds = isWin && wantKiosk ? screen.getPrimaryDisplay().bounds : undefined

  const savedBounds = compositorMode ? undefined : sanitizeBounds(readMainBounds(runtimeState))

  mainWindow = new BrowserWindow({
    width: winKioskBounds?.width ?? savedBounds?.width ?? runtimeState.config.displayWidth,
    height: winKioskBounds?.height ?? savedBounds?.height ?? runtimeState.config.displayHeight,
    x: winKioskBounds?.x ?? savedBounds?.x,
    y: winKioskBounds?.y ?? savedBounds?.y,
    frame: !compositorMode && !isWin,
    resizable: true,
    useContentSize: true,
    kiosk: false,
    autoHideMenuBar: true,
    transparent: compositorMode || isWin,
    backgroundColor: transparentWindow ? '#00000000' : '#000',
    fullscreenable: true,
    simpleFullscreen: false,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: false,
      nodeIntegration: false,
      contextIsolation: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      backgroundThrottling: false
    }
  })

  // Re-apply bounds after the compositor shows the window. Skip on Windows kiosk: that window
  // is intentionally created full-screen and must not be resized back to a saved windowed size.
  if (savedBounds && !winKioskBounds) {
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
    if (compositorMode) return
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
  attachResizeReflow()

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

    const baseW = savedBounds?.width || runtimeState.config.displayWidth || 1200
    const baseH = savedBounds?.height || runtimeState.config.displayHeight || 720

    // Windows kiosk is created already full-screen (no resize, see winKioskBounds); everyone
    // else starts windowed. In compositor mode the compositor owns the size (tiled toplevel).
    if (!winKioskBounds && !compositorMode) applyWindowedContentSize(mainWindow, baseW, baseH)
    mainWindow.show()

    // Snapshot the geometry
    scheduleMainBoundsSave()

    const forceKiosk = process.env.LIVI_KIOSK === '1'
    if (runtimeState.config.kiosk?.main || forceKiosk) {
      const goFullscreen = () => {
        if (!mainWindow || mainWindow.isDestroyed()) return

        const d = screen.getDisplayMatching(mainWindow.getBounds())
        const [cw, ch] = mainWindow.getContentSize()
        console.log(
          `[kiosk] enter: screen=${d.size.width}x${d.size.height} ` +
            `workArea=${d.workAreaSize.width}x${d.workAreaSize.height} window=${cw}x${ch}`
        )

        if (isMac) {
          mainWindow.setFullScreen(true)
        } else if (compositorMode) {
          mainWindow.setContentSize(d.size.width, d.size.height)
          mainWindow.setFullScreen(true)
        } else if (isWin) {
          // Transparent window: don't resize into fullscreen (electron/electron#49173). The
          // window was already created at the display size; just cover the full display (incl.
          // the taskbar area) and lift above the always-on-top taskbar.
          mainWindow.setBounds(d.bounds)
          mainWindow.setAlwaysOnTop(true, 'screen-saver')
        } else {
          mainWindow.setKiosk(true)
          mainWindow.setContentSize(d.workAreaSize.width, d.workAreaSize.height)
        }
      }

      if (compositorMode) {
        // The nested compositor only learns the monitor size once the host fullscreens
        // the output. Going fullscreen at ready-to-show is too early: the output/host
        // handshake hasn't settled, so the UI ends up sized to the windowed mode inside
        // the fullscreen window. Defer a beat so the output is established first.
        setTimeout(goFullscreen, 400)
      } else {
        setImmediate(goFullscreen)
      }
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
        runtimeState.config.displayWidth || 800,
        runtimeState.config.displayHeight || 480
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
        runtimeState.config.displayWidth || 800,
        runtimeState.config.displayHeight || 480
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
