import { saveConfig } from '@main/core'
import { customProxy } from '@main/services/custom/CustomProxy'
import { runtimeStateProps } from '@main/types'
import { isDev, isMacPlatform } from '@main/utils'
import { drawVideoIn } from '@main/video'
import type { WindowBounds } from '@shared/types'
import { app, BrowserWindow, screen, session, shell } from 'electron'
import { join } from 'path'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  attachKioskStateSync,
  attachResizeReflow,
  persistKiosk,
  sanitizeBounds,
  uiZoomFactor
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

export function createMainWindow(runtimeState: runtimeStateProps) {
  const isMac = isMacPlatform()
  // A checkout started by core runs the built UI, the dev tools belong to the Vite server.
  const devServer = isDev() ? process.env.ELECTRON_RENDERER_URL : undefined
  const compositorMode = process.env.LIVI_COMPOSITOR === '1'
  const transparentWindow = compositorMode || isMac

  const savedBounds = compositorMode ? undefined : sanitizeBounds(readMainBounds(runtimeState))

  mainWindow = new BrowserWindow({
    width: savedBounds?.width ?? runtimeState.config.mainScreenWidth,
    height: savedBounds?.height ?? runtimeState.config.mainScreenHeight,
    x: savedBounds?.x,
    y: savedBounds?.y,
    frame: !compositorMode,
    resizable: true,
    useContentSize: true,
    kiosk: false,
    autoHideMenuBar: true,
    transparent: compositorMode,
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

  drawVideoIn('main', mainWindow)
  mainWindow.on('closed', () => drawVideoIn('main', null))

  // The window manager may move the window when it first shows it.
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

  let boundsTimer: NodeJS.Timeout | null = null
  const persistMainBounds = () => {
    if (!mainWindow || mainWindow.isDestroyed()) return
    if (compositorMode) return
    try {
      if (mainWindow.isFullScreen()) return
      if (typeof mainWindow.isKiosk === 'function' && mainWindow.isKiosk()) return
    } catch {}
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
    saveConfig({ mainScreenBounds: next })
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

  attachKioskStateSync(runtimeState)
  attachResizeReflow()

  const ses = mainWindow.webContents.session
  ses.setPermissionCheckHandler((_w, p) => ['media', 'display-capture'].includes(p))
  ses.setPermissionRequestHandler((_w, p, cb) => cb(['media', 'display-capture'].includes(p)))

  session.defaultSession.webRequest.onHeadersReceived(
    { urls: ['*://*/*', 'file://*/*'] },
    (d, cb) => {
      const proxied = customProxy.url()
      if (proxied && d.url.startsWith(proxied)) return cb({})
      cb({
        responseHeaders: {
          ...d.responseHeaders,
          'Cross-Origin-Opener-Policy': ['same-origin'],
          'Cross-Origin-Embedder-Policy': ['require-corp'],
          'Cross-Origin-Resource-Policy': ['same-site']
        }
      })
    }
  )

  mainWindow.once('ready-to-show', () => {
    const win = mainWindow as BrowserWindow

    const shapeW = runtimeState.config.mainScreenWidth || 1200
    const shapeH = runtimeState.config.mainScreenHeight || 720
    const want = savedBounds ?? { width: shapeW, height: shapeH }

    // In compositor mode the compositor owns the size.
    if (!compositorMode) applyWindowedContentSize(win, shapeW, shapeH, want)
    win.show()

    scheduleMainBoundsSave()

    const forceKiosk = process.env.LIVI_KIOSK === '1'
    if (runtimeState.config.kiosk?.main || forceKiosk) {
      const goFullscreen = () => {
        if (win.isDestroyed()) return

        const d = screen.getDisplayMatching(win.getBounds())
        const [cw, ch] = win.getContentSize()

        console.log(
          `[kiosk] enter: screen=${d.size.width}x${d.size.height} ` +
            `workArea=${d.workAreaSize.width}x${d.workAreaSize.height} window=${cw}x${ch}`
        )

        if (isMac) {
          win.setFullScreen(true)
        } else if (compositorMode) {
          win.setContentSize(d.size.width, d.size.height)
          win.setFullScreen(true)
        } else {
          win.setKiosk(true)
          win.setContentSize(d.workAreaSize.width, d.workAreaSize.height)
        }
      }

      if (compositorMode) {
        // The nested compositor learns the monitor size only after the host fullscreens the
        // output. Going fullscreen at ready-to-show leaves the UI at the windowed size.
        setTimeout(goFullscreen, 400)
      } else {
        setImmediate(goFullscreen)
      }
    }

    win.webContents.setZoomFactor(
      uiZoomFactor(runtimeState.config.uiZoomPercent, win.getContentSize()[1])
    )

    if (devServer) {
      win.webContents.openDevTools({ mode: 'detach' })
    }
  })

  if (isMac) {
    mainWindow.on('enter-full-screen', () => {
      if (runtimeState.suppressNextFsSync) return
      applyAspectRatioFullscreen(
        mainWindow!,
        runtimeState.config.mainScreenWidth || 800,
        runtimeState.config.mainScreenHeight || 480
      )
      persistKiosk(true, runtimeState)
    })

    mainWindow.on('leave-full-screen', () => {
      if (runtimeState.suppressNextFsSync) {
        runtimeState.suppressNextFsSync = false
        return
      }
      applyAspectRatioWindowed(
        mainWindow!,
        runtimeState.config.mainScreenWidth || 800,
        runtimeState.config.mainScreenHeight || 480
      )
      persistKiosk(false, runtimeState)
    })
  }

  mainWindow.webContents.on('render-process-gone', (_e, details) => {
    console.error(`[window] renderer gone: reason=${details.reason} exitCode=${details.exitCode}`)
  })

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  if (devServer) {
    mainWindow.loadURL(devServer)
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

  if (devServer) {
    const gpuWindow = new BrowserWindow({
      width: 1000,
      height: 800,
      title: 'GPU Info',
      webPreferences: { nodeIntegration: false, contextIsolation: true }
    })
    gpuWindow.loadURL('chrome://gpu')
  }

  if (devServer) {
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
