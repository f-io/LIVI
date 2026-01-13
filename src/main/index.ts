import { app, shell, BrowserWindow, session, ipcMain, protocol } from 'electron'
import { join, extname, dirname, basename } from 'path'
import {
  existsSync,
  createReadStream,
  readFileSync,
  writeFileSync,
  createWriteStream,
  promises as fsp
} from 'fs'
import { electronApp, is } from '@electron-toolkit/utils'
import { DEFAULT_CONFIG } from './carplay/driver/DongleDriver'
import { ICON_120_B64, ICON_180_B64, ICON_256_B64 } from './carplay/assets/carIcons'
import { Socket } from './Socket'
import { ExtraConfig, DEFAULT_BINDINGS } from './Globals'
import { USBService } from './usb/USBService'
import { CarplayService } from './carplay/services/CarplayService'
import { execFile, spawn } from 'node:child_process'
import os from 'node:os'
import https from 'node:https'

function setFeatureFlags(flags: string[]) {
  app.commandLine.appendSwitch('enable-features', flags.join(','))
}

function linuxPresetAngleVulkan() {
  app.commandLine.appendSwitch('use-gl', 'angle')
  app.commandLine.appendSwitch('use-angle', 'vulkan')
  setFeatureFlags([
    'Vulkan',
    'VulkanFromANGLE',
    'DefaultANGLEVulkan',
    'UnsafeWebGPU',
    'AcceleratedVideoDecodeLinuxZeroCopyGL',
    'AcceleratedVideoEncoder',
    'VaapiIgnoreDriverChecks',
    'UseMultiPlaneFormatForHardwareVideo'
  ])
  app.commandLine.appendSwitch('ozone-platform-hint', 'auto')
}

function commonGpuToggles() {
  app.commandLine.appendSwitch('ignore-gpu-blocklist')
  app.commandLine.appendSwitch('enable-gpu-rasterization')
  app.commandLine.appendSwitch('disable-features', 'UseChromeOSDirectVideoDecoder')
}

// Linux x64 -> ANGLE + Vulkan + WebGPU
if (process.platform === 'linux' && process.arch === 'x64') {
  commonGpuToggles()
  linuxPresetAngleVulkan()
  app.commandLine.appendSwitch('enable-unsafe-webgpu')
  app.commandLine.appendSwitch('enable-dawn-features', 'allow_unsafe_apis')
}

// macOS: WebGPU
if (process.platform === 'darwin') {
  app.commandLine.appendSwitch('enable-unsafe-webgpu')
  app.commandLine.appendSwitch('enable-dawn-features', 'allow_unsafe_apis')
}

app.on('gpu-info-update', () => {
  console.log('GPU Info:', app.getGPUFeatureStatus())
})

const mimeTypeFromExt = (ext: string): string =>
  (
    ({
      '.html': 'text/html',
      '.js': 'text/javascript',
      '.css': 'text/css',
      '.png': 'image/png',
      '.jpg': 'image/jpeg',
      '.jpeg': 'image/jpeg',
      '.svg': 'image/svg+xml',
      '.json': 'application/json',
      '.wasm': 'application/wasm',
      '.map': 'application/json'
    }) as const
  )[ext.toLowerCase()] ?? 'application/octet-stream'

const MIN_WIDTH = 400
const isMac = process.platform === 'darwin'

function applyAspectRatioWindowed(win: BrowserWindow, width: number, height: number): void {
  const ratio = width && height ? width / height : 0
  const [winW, winH] = win.getSize()
  const [contentW, contentH] = win.getContentSize()
  const extraWidth = Math.max(0, winW - contentW)
  const extraHeight = Math.max(0, winH - contentH)
  win.setAspectRatio(ratio, { width: extraWidth, height: extraHeight })
  if (ratio > 0) {
    const minH = Math.round(MIN_WIDTH / ratio)
    win.setMinimumSize(MIN_WIDTH + extraWidth, minH + extraHeight)
  } else {
    win.setMinimumSize(0, 0)
  }
}
function applyAspectRatioFullscreen(win: BrowserWindow, width: number, height: number): void {
  const ratio = width && height ? width / height : 0
  win.setAspectRatio(ratio, { width: 0, height: 0 })
}

// Globals
let mainWindow: BrowserWindow | null
let socket: Socket
let config: ExtraConfig
let usbService: USBService
let isQuitting = false
let suppressNextFsSync = false

const carplayService = new CarplayService()
declare global {
  var carplayService: CarplayService | undefined
}
globalThis.carplayService = carplayService

type UpdateSessionState = 'idle' | 'downloading' | 'ready' | 'installing'
let updateSession: {
  state: UpdateSessionState
  tmpFile?: string
  cancel?: () => void
  platform?: 'darwin' | 'linux'
} = { state: 'idle' }

type UpdateEventPayload =
  | { phase: 'start' }
  | { phase: 'download'; received: number; total: number; percent: number }
  | { phase: 'ready' }
  | { phase: 'mounting' | 'copying' | 'unmounting' | 'installing' | 'relaunching' }
  | { phase: 'error'; message: string }

// GitHub API
interface GhAsset {
  name?: string
  browser_download_url?: string
}
interface GhRelease {
  tag_name?: string
  name?: string
  assets?: GhAsset[]
}

const NULL_DELETES: (keyof ExtraConfig)[] = [
  'primaryColorDark',
  'primaryColorLight',
  'highlightColorDark',
  'highlightColorLight'
  // add more explicit “reset-to-default” keys here
]

function applyNullDeletes(merged: ExtraConfig, next: Partial<ExtraConfig>) {
  for (const key of NULL_DELETES) {
    if ((next as any)[key] === null) {
      delete (merged as any)[key]
    }
  }
}

app.on('before-quit', async (e) => {
  if (isQuitting) return
  isQuitting = true
  e.preventDefault()

  const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms))

  const withTimeout = async <T>(
    label: string,
    p: Promise<T>,
    ms: number
  ): Promise<T | undefined> => {
    let t: NodeJS.Timeout | null = null
    try {
      return (await Promise.race([
        p,
        new Promise<T | undefined>((resolve) => {
          t = setTimeout(() => {
            console.warn(`[MAIN] before-quit timeout: ${label} after ${ms}ms`)
            resolve(undefined)
          }, ms)
        })
      ])) as T | undefined
    } finally {
      if (t) clearTimeout(t)
    }
  }

  const measureStep = async (label: string, fn: () => Promise<unknown>) => {
    const t0 = Date.now()
    console.log(`[MAIN] before-quit step:start ${label}`)
    try {
      await fn()
    } finally {
      console.log(`[MAIN] before-quit step:done ${label} (${Date.now() - t0}ms)`)
    }
  }

  // Safeguards based on measured timings
  const tUsbStop = 500
  const tDisconnect = 800
  const tCarplayStop = 6000

  // Global watchdog: log only
  const watchdogMs = process.platform === 'darwin' ? 10000 : 3000
  const watchdog = setTimeout(() => {
    console.warn(`[MAIN] before-quit watchdog: giving up waiting after ${watchdogMs}ms`)
  }, watchdogMs)

  try {
    ;(carplayService as any).shuttingDown = true

    // Block hotplug callbacks ASAP
    usbService?.beginShutdown()

    await measureStep('usbService.stop()', async () => {
      await withTimeout('usbService.stop()', usbService?.stop?.() ?? Promise.resolve(), tUsbStop)
    })

    await measureStep('carplay.disconnectPhone()', async () => {
      await withTimeout('carplay.disconnectPhone()', carplayService.disconnectPhone(), tDisconnect)
      await sleep(75)
    })

    await measureStep('carplay.stop()', async () => {
      await withTimeout('carplay.stop()', carplayService.stop(), tCarplayStop)
    })
  } catch (err) {
    console.warn('[MAIN] Error while quitting:', err)
  } finally {
    setTimeout(() => clearTimeout(watchdog), 250)
    setImmediate(() => app.quit())
  }
})

// Protocol & Config
protocol.registerSchemesAsPrivileged([
  {
    scheme: 'app',
    privileges: {
      secure: true,
      standard: true,
      corsEnabled: true,
      supportFetchAPI: true,
      stream: true
    }
  }
])

const appPath = app.getPath('userData')
const configPath = join(appPath, 'config.json')

function loadConfig(): ExtraConfig {
  let fileConfig: Partial<ExtraConfig> = {}
  if (existsSync(configPath)) {
    try {
      fileConfig = JSON.parse(readFileSync(configPath, 'utf8'))
    } catch (e) {
      console.warn('[config] Failed to parse config.json, using defaults:', e)
      fileConfig = {}
    }
  }

  // Start with defaults
  const merged: ExtraConfig = {
    ...DEFAULT_CONFIG,
    kiosk: true,
    camera: '',
    nightMode: true,
    audioVolume: 1.0,
    navVolume: 1.0,
    siriVolume: 1.0,
    callVolume: 1.0,
    visualAudioDelayMs: 120,
    ...fileConfig,
    bindings: { ...DEFAULT_BINDINGS, ...(fileConfig.bindings || {}) }
  } as ExtraConfig

  if (!merged.dongleIcon120) merged.dongleIcon120 = ICON_120_B64
  if (!merged.dongleIcon180) merged.dongleIcon180 = ICON_180_B64
  if (!merged.dongleIcon256) merged.dongleIcon256 = ICON_256_B64

  const needWrite = !existsSync(configPath) || JSON.stringify(fileConfig) !== JSON.stringify(merged)

  if (needWrite) {
    try {
      writeFileSync(configPath, JSON.stringify(merged, null, 2))
      console.log('[config] Written complete config.json with all defaults')
    } catch (e) {
      console.warn('[config] Failed to write config.json:', e)
    }
  }

  return merged
}
config = loadConfig()

// Updater helpers
function pickAssetForPlatform(assets: GhAsset[]): { url?: string } {
  if (!Array.isArray(assets)) return {}

  const nameOf = (a: GhAsset) => a?.name || a?.browser_download_url || ''
  const urlOf = (a?: GhAsset) => a?.browser_download_url

  if (process.platform === 'darwin') {
    const dmgs = assets.filter((a) => /\.dmg$/i.test(nameOf(a)))
    if (dmgs.length === 0) return {}
    const arch = process.arch
    const preferred =
      arch === 'arm64'
        ? (dmgs.find((a) => /(arm64|aarch64|apple[-_]?silicon|universal)/i.test(nameOf(a))) ??
          dmgs[0])
        : (dmgs.find((a) => /(x86_64|amd64|x64|universal)/i.test(nameOf(a))) ?? dmgs[0])
    return { url: urlOf(preferred) }
  }

  if (process.platform === 'linux') {
    const appImages = assets.filter((a) => /\.AppImage$/i.test(nameOf(a)))
    if (appImages.length === 0) return {}
    let patterns: RegExp[] = []
    if (process.arch === 'x64') {
      patterns = [/[-_.]x86_64\.AppImage$/i, /[-_.]amd64\.AppImage$/i, /[-_.]x64\.AppImage$/i]
    } else if (process.arch === 'arm64') {
      patterns = [/[-_.]arm64\.AppImage$/i, /[-_.]aarch64\.AppImage$/i]
    } else {
      return {}
    }
    const match = appImages.find((a) => patterns.some((re) => re.test(nameOf(a))))
    return { url: urlOf(match) }
  }

  return {}
}

function sendUpdateEvent(payload: UpdateEventPayload) {
  mainWindow?.webContents.send('update:event', payload)
}
function sendUpdateProgress(payload: Extract<UpdateEventPayload, { phase: 'download' }>) {
  mainWindow?.webContents.send('update:progress', payload)
}

function downloadWithProgress(
  url: string,
  dest: string,
  onProgress: (p: { received: number; total: number; percent: number }) => void
): { promise: Promise<void>; cancel: () => void } {
  let req: import('http').ClientRequest | null = null
  let file: import('fs').WriteStream | null = null
  let resolved = false
  let rejected = false
  let cancelled = false
  let _resolve: (() => void) | null = null
  let _reject: ((e: unknown) => void) | null = null
  let redirectCancel: (() => void) | null = null

  const safeResolve = () => {
    if (!resolved && !rejected) {
      resolved = true
      _resolve?.()
    }
  }
  const safeReject = (e: unknown) => {
    if (!resolved && !rejected) {
      rejected = true
      _reject?.(e)
    }
  }

  const cleanup = async () => {
    try {
      req?.destroy()
    } catch {}
    req = null
    try {
      file?.destroy()
    } catch {}
    file = null
    try {
      if (existsSync(dest)) await fsp.unlink(dest).catch(() => {})
    } catch {}
  }

  const promise = new Promise<void>((resolve, reject) => {
    _resolve = resolve
    _reject = (e: unknown) => reject(e)

    req = https.get(url, (res) => {
      // Redirect
      if (res.statusCode && res.statusCode >= 300 && res.headers.location) {
        try {
          req!.destroy()
        } catch {}
        const next = downloadWithProgress(res.headers.location, dest, onProgress)
        redirectCancel = next.cancel
        next.promise.then(resolve, reject)
        req = null
        return
      }

      if (res.statusCode !== 200) {
        safeReject(new Error(`HTTP ${res.statusCode}`))
        return
      }

      const total = parseInt(String(res.headers['content-length'] || 0), 10) || 0
      let received = 0
      file = createWriteStream(dest)

      res.on('data', (chunk: Buffer) => {
        if (cancelled) return
        received += chunk.length
        onProgress({ received, total, percent: total ? received / total : 0 })
      })
      res.on('error', (err: Error) => {
        if (cancelled) return
        safeReject(err)
      })

      file.on('error', (err: Error) => {
        if (cancelled) return
        safeReject(err)
      })
      file.on('finish', async () => {
        if (cancelled) return
        try {
          await new Promise<void>((r) => file?.close(() => r()))
        } catch {}
        file = null
        safeResolve()
      })

      res.pipe(file)
    })

    req.on('error', (e: Error) => {
      if (cancelled) return
      safeReject(e)
    })
  })

  const cancel = () => {
    if (cancelled) return
    cancelled = true
    try {
      redirectCancel?.()
    } catch {}
    cleanup().finally(() => safeReject(new Error('aborted')))
  }

  return { promise, cancel }
}

async function getMacDesiredOwner(dstApp: string): Promise<{ user: string; group: string }> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  if (existsSync(dstApp)) {
    try {
      const out = await new Promise<string>((resolve, reject) =>
        execFile('stat', ['-f', '%Su:%Sg', dstApp], (err, stdout) =>
          err ? reject(err) : resolve(stdout.trim())
        )
      )
      const [user, group] = out.split(':')
      if (user) return { user, group: group || 'staff' }
    } catch {}
  }
  const user = process.env.SUDO_USER || process.env.USER || os.userInfo().username
  let group = 'staff'
  try {
    const groups = await new Promise<string>((resolve, reject) =>
      execFile('id', ['-Gn', user], (err, stdout) => (err ? reject(err) : resolve(stdout.trim())))
    )
    if (groups.split(/\s+/).includes('admin')) group = 'admin'
  } catch {}
  return { user, group }
}

async function installFromDmg(dmgPath: string): Promise<void> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  const mountPoint = `/Volumes/pcu-${Date.now()}`
  sendUpdateEvent({ phase: 'mounting' })
  await new Promise<void>((resolve, reject) =>
    execFile('hdiutil', ['attach', '-nobrowse', '-mountpoint', mountPoint, dmgPath], (err) =>
      err ? reject(err) : resolve()
    )
  )

  const entries = await fsp.readdir(mountPoint, { withFileTypes: true })
  const appFolder = entries.find(
    (e) => e.isDirectory() && e.name.toLowerCase().endsWith('.app')
  )?.name
  if (!appFolder) {
    await new Promise<void>((resolve) =>
      execFile('hdiutil', ['detach', mountPoint, '-quiet'], () => resolve())
    )
    throw new Error('No .app found in DMG')
  }

  const srcApp = join(mountPoint, appFolder)
  const dstApp = '/Applications/pi-carplay.app'
  const desired = await getMacDesiredOwner(dstApp)

  sendUpdateEvent({ phase: 'copying' })
  const script =
    `do shell script "set -e; dst=\\"${dstApp}\\"; src=\\"${srcApp}\\"; ` +
    `chflags -R nouchg,noschg $dst 2>/dev/null || true; rm -rf $dst; ` +
    `ditto -v $src $dst; xattr -cr $dst; chmod -RN $dst 2>/dev/null || true; ` +
    `chflags -R nouchg,noschg $dst 2>/dev/null || true; chown -R ${desired.user}:${desired.group} $dst" with administrator privileges`
  await new Promise<void>((resolve, reject) =>
    execFile('osascript', ['-e', script], (err) => (err ? reject(err) : resolve()))
  )

  sendUpdateEvent({ phase: 'unmounting' })
  await new Promise<void>((resolve) =>
    execFile('hdiutil', ['detach', mountPoint, '-quiet'], () => resolve())
  )
}

async function installOnMacFromFile(dmgPath: string): Promise<void> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  sendUpdateEvent({ phase: 'installing' })
  await installFromDmg(dmgPath)
  sendUpdateEvent({ phase: 'relaunching' })
  app.relaunch()
  setImmediate(() => app.quit())
}

async function installOnLinuxFromFile(appImagePath: string): Promise<void> {
  if (process.platform !== 'linux') throw new Error('Linux only')
  const current = process.env.APPIMAGE
  if (!current) throw new Error('Not running from an AppImage')

  const currentDir = dirname(current)
  const currentBase = basename(current)

  const destNew = join(currentDir, currentBase + '.new')
  await fsp.copyFile(appImagePath, destNew)
  await fsp.chmod(destNew, 0o755)
  await fsp.rename(destNew, current)

  sendUpdateEvent({ phase: 'relaunching' })

  const cleanEnv: Record<string, string | undefined> = { ...process.env }
  delete cleanEnv.APPIMAGE
  delete cleanEnv.APPDIR
  delete cleanEnv.ARGV0
  delete cleanEnv.OWD

  const child = spawn(current, [], { detached: true, stdio: 'ignore', env: cleanEnv })
  child.unref()
  app.quit()
}

function sendKioskSync(kiosk: boolean) {
  mainWindow?.webContents.send('settings:kiosk-sync', kiosk)
}

function persistKioskAndBroadcast(kiosk: boolean) {
  if (config.kiosk === kiosk) return
  saveSettings({ kiosk })
}

function currentKiosk(): boolean {
  const win = mainWindow
  if (win && !win.isDestroyed()) {
    return isMac ? win.isFullScreen() : win.isKiosk()
  }
  return !!config.kiosk
}

function applyWindowedContentSize(win: BrowserWindow, w: number, h: number) {
  win.setContentSize(w, h, false)
  applyAspectRatioWindowed(win, w, h)
}

function createWindow(): void {
  mainWindow = new BrowserWindow({
    width: config.width,
    height: config.height,
    frame: isMac ? true : !config.kiosk,
    useContentSize: true,
    kiosk: isMac ? false : !!config.kiosk,
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
      const baseW = config.width || 800
      const baseH = config.height || 480
      applyWindowedContentSize(mainWindow, baseW, baseH)
      mainWindow.show()
      if (config.kiosk) setImmediate(() => mainWindow!.setFullScreen(true))
    } else {
      if (config.kiosk) {
        mainWindow.setKiosk(true)
        applyAspectRatioWindowed(mainWindow, 0, 0)
      } else {
        mainWindow.setContentSize(config.width, config.height, false)
        applyAspectRatioWindowed(mainWindow, config.width, config.height)
      }
      mainWindow.show()
    }

    sendKioskSync(currentKiosk())

    if (is.dev) mainWindow.webContents.openDevTools({ mode: 'detach' })
    carplayService.attachRenderer(mainWindow.webContents)
  })

  if (isMac) {
    mainWindow.on('enter-full-screen', () => {
      if (suppressNextFsSync) return
      applyAspectRatioFullscreen(mainWindow!, config.width || 800, config.height || 480)
      persistKioskAndBroadcast(true)
    })

    mainWindow.on('leave-full-screen', () => {
      if (suppressNextFsSync) {
        suppressNextFsSync = false
        return
      }
      applyAspectRatioWindowed(mainWindow!, config.width || 800, config.height || 480)
      persistKioskAndBroadcast(false)
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
        suppressNextFsSync = true
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

// App lifecycle
app.whenReady().then(() => {
  electronApp.setAppUserModelId('com.electron.carplay')

  protocol.registerStreamProtocol('app', (request, cb) => {
    try {
      const u = new URL(request.url)
      let path = decodeURIComponent(u.pathname)
      if (path === '/' || path === '') path = '/index.html'
      const file = join(__dirname, '../renderer', path)
      if (!existsSync(file)) return cb({ statusCode: 404 })
      cb({
        statusCode: 200,
        headers: {
          'Content-Type': mimeTypeFromExt(extname(file)),
          'Cross-Origin-Opener-Policy': 'same-origin',
          'Cross-Origin-Embedder-Policy': 'require-corp',
          'Cross-Origin-Resource-Policy': 'same-site'
        },
        data: createReadStream(file)
      })
    } catch (e) {
      console.error('[app-protocol] error', e)
      cb({ statusCode: 500 })
    }
  })

  usbService = new USBService(carplayService)
  socket = new Socket(config, saveSettings)

  ipcMain.handle('quit', () =>
    isMac
      ? mainWindow?.isFullScreen()
        ? (() => {
            suppressNextFsSync = true
            mainWindow!.once('leave-full-screen', () => mainWindow?.hide())
            mainWindow!.setFullScreen(false)
          })()
        : mainWindow?.hide()
      : app.quit()
  )

  ipcMain.handle('settings:get-kiosk', () => currentKiosk())
  ipcMain.handle('getSettings', () => config)
  ipcMain.handle('save-settings', (_evt, settings: Partial<ExtraConfig>) => {
    saveSettings(settings)
    return true
  })
  ipcMain.handle('settings:reset-dongle-icons', () => {
    const next: ExtraConfig = {
      ...config,
      dongleIcon120: ICON_120_B64,
      dongleIcon180: ICON_180_B64,
      dongleIcon256: ICON_256_B64
    }

    saveSettings(next)

    return {
      dongleIcon120: next.dongleIcon120,
      dongleIcon180: next.dongleIcon180,
      dongleIcon256: next.dongleIcon256
    }
  })

  ipcMain.handle('app:getVersion', () => app.getVersion())

  ipcMain.handle('app:getLatestRelease', async () => {
    try {
      const repo = process.env.UPDATE_REPO || 'f-io/pi-carplay'
      const feed = process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
      const res = await fetch(feed, { headers: { 'User-Agent': 'pi-carplay-updater' } })
      if (!res.ok) throw new Error(`feed ${res.status}`)
      const json = (await res.json()) as unknown as GhRelease
      const raw = (json.tag_name || json.name || '').toString()
      const version = raw.replace(/^v/i, '')
      const { url } = pickAssetForPlatform(json.assets || [])
      return { version, url }
    } catch (e) {
      console.warn('[update] getLatestRelease failed:', e)
      return { version: '', url: undefined }
    }
  })

  ipcMain.handle('app:performUpdate', async (_evt, directUrl?: string) => {
    try {
      if (updateSession.state !== 'idle') throw new Error('Update already in progress')
      sendUpdateEvent({ phase: 'start' })

      const platform = process.platform
      if (platform !== 'darwin' && platform !== 'linux') {
        sendUpdateEvent({ phase: 'error', message: 'Unsupported platform' })
        return
      }
      updateSession.platform = platform as 'darwin' | 'linux'

      let url = directUrl
      if (!url) {
        const repo = process.env.UPDATE_REPO || 'f-io/pi-carplay'
        const feed =
          process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
        const res = await fetch(feed, { headers: { 'User-Agent': 'pi-carplay-updater' } })
        if (!res.ok) throw new Error(`feed ${res.status}`)
        const json = (await res.json()) as unknown as GhRelease
        url = pickAssetForPlatform(json.assets || []).url
      }
      if (!url) throw new Error('No asset found for platform')

      const suffix = platform === 'darwin' ? '.dmg' : '.AppImage'
      const tmpFile = join(os.tmpdir(), `pcu-${Date.now()}${suffix}`)
      updateSession.tmpFile = tmpFile

      updateSession.state = 'downloading'
      const { promise, cancel } = downloadWithProgress(
        url,
        tmpFile,
        ({ received, total, percent }) => {
          sendUpdateProgress({ phase: 'download', received, total, percent })
        }
      )
      updateSession.cancel = () => {
        cancel()
        updateSession = { state: 'idle' }
        sendUpdateEvent({ phase: 'error', message: 'Aborted' })
      }

      await promise
      updateSession.state = 'ready'
      sendUpdateEvent({ phase: 'ready' })
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      updateSession = { state: 'idle' }
      sendUpdateEvent({ phase: 'error', message: msg })
    }
  })

  ipcMain.handle('app:abortUpdate', async () => {
    try {
      if (updateSession.state === 'downloading' && updateSession.cancel) {
        updateSession.cancel()
      } else if (updateSession.state === 'ready') {
        if (updateSession.tmpFile && existsSync(updateSession.tmpFile)) {
          try {
            await fsp.unlink(updateSession.tmpFile)
          } catch {}
        }
      }
    } finally {
      updateSession = { state: 'idle' }
      sendUpdateEvent({ phase: 'error', message: 'Aborted' })
    }
  })

  ipcMain.handle('app:beginInstall', async () => {
    try {
      if (updateSession.state !== 'ready' || !updateSession.tmpFile || !updateSession.platform) {
        throw new Error('No downloaded update ready')
      }
      const file = updateSession.tmpFile
      updateSession.state = 'installing'
      if (updateSession.platform === 'darwin') await installOnMacFromFile(file)
      else await installOnLinuxFromFile(file)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      updateSession = { state: 'idle' }
      sendUpdateEvent({ phase: 'error', message: msg })
    }
  })

  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0 && !mainWindow) createWindow()
    else mainWindow?.show()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})

function sizesEqual(a: ExtraConfig, b: ExtraConfig) {
  const aw = Number(a.width) || 0
  const ah = Number(a.height) || 0
  const bw = Number(b.width) || 0
  const bh = Number(b.height) || 0
  return aw === bw && ah === bh
}

// Settings IPC
function saveSettings(next: Partial<ExtraConfig>) {
  const merged: ExtraConfig = {
    ...config,
    ...next,
    bindings: {
      ...DEFAULT_BINDINGS,
      ...(config.bindings ?? {}),
      ...(next.bindings ?? {})
    }
  } as ExtraConfig

  applyNullDeletes(merged, next)

  try {
    writeFileSync(configPath, JSON.stringify(merged, null, 2))
  } catch (e) {
    console.warn('[config] saveSettings failed:', e)
  }

  const prev = config
  config = merged

  if (socket) {
    socket.config = config
    socket.sendSettings()
  }
  sendKioskSync(config.kiosk)

  if (!mainWindow) return

  const sizeChanged = !sizesEqual(prev, config)
  const kioskChanged = prev.kiosk !== config.kiosk

  if (process.platform === 'darwin') {
    const wantFs = !!config.kiosk
    const isFs = mainWindow.isFullScreen()

    if (kioskChanged) {
      if (wantFs) {
        if (sizeChanged) {
          applyWindowedContentSize(mainWindow, config.width || 800, config.height || 480)
          applyAspectRatioFullscreen(mainWindow, config.width || 800, config.height || 480)
        }
        if (!isFs) mainWindow.setFullScreen(true)
      } else {
        if (isFs) mainWindow.setFullScreen(false)
        if (sizeChanged) {
          applyWindowedContentSize(mainWindow, config.width || 800, config.height || 480)
        }
      }
    } else if (sizeChanged) {
      if (wantFs) {
        applyWindowedContentSize(mainWindow, config.width || 800, config.height || 480)
        applyAspectRatioFullscreen(mainWindow, config.width || 800, config.height || 480)
      } else {
        applyWindowedContentSize(mainWindow, config.width || 800, config.height || 480)
      }
    }
  } else {
    // Linux
    if (kioskChanged) {
      mainWindow.setKiosk(!!config.kiosk)
      if (sizeChanged) {
        if (config.kiosk) {
          applyAspectRatioWindowed(mainWindow, 0, 0)
        } else {
          mainWindow.setContentSize(config.width, config.height, false)
          applyAspectRatioWindowed(mainWindow, config.width, config.height)
        }
      }
    } else if (sizeChanged) {
      if (config.kiosk) {
        applyAspectRatioWindowed(mainWindow, 0, 0)
      } else {
        mainWindow.setContentSize(config.width, config.height, false)
        applyAspectRatioWindowed(mainWindow, config.width, config.height)
      }
    }
  }
}
