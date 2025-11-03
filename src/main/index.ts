import { app, shell, BrowserWindow, session, ipcMain, protocol } from 'electron'
import { join, extname, dirname, basename } from 'path'
import {
  existsSync,
  createReadStream,
  readFileSync,
  writeFileSync,
  createWriteStream,
  promises as fsp,
} from 'fs'
import { electronApp, is } from '@electron-toolkit/utils'
import { DEFAULT_CONFIG } from '@carplay/node'
import { Socket } from './Socket'
import { ExtraConfig, KeyBindings } from './Globals'
import { USBService } from './usb/USBService'
import { CarplayService } from './carplay/CarplayService'
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
    'AcceleratedVideoDecodeLinuxZeroCopyGL',
    'AcceleratedVideoEncoder',
    'VaapiIgnoreDriverChecks',
    'UseMultiPlaneFormatForHardwareVideo',
  ])
  app.commandLine.appendSwitch('ozone-platform-hint', 'auto')
}
function linuxPresetEglGl() {
  app.commandLine.appendSwitch('use-gl', 'egl')
  setFeatureFlags([
    'AcceleratedVideoDecodeLinuxGL',
    'AcceleratedVideoDecodeLinuxZeroCopyGL',
    'AcceleratedVideoEncoder',
    'UseMultiPlaneFormatForHardwareVideo',
  ])
}
function commonGpuToggles() {
  app.commandLine.appendSwitch('ignore-gpu-blocklist')
  app.commandLine.appendSwitch('enable-gpu-rasterization')
  app.commandLine.appendSwitch('disable-features', 'UseChromeOSDirectVideoDecoder')
}

if (process.platform === 'linux' && process.arch === 'x64') {
  commonGpuToggles()
  linuxPresetAngleVulkan()
  if (process.env.HW_DEBUG === 'egl') linuxPresetEglGl()
}
if (process.platform === 'darwin') {
  app.commandLine.appendSwitch('enable-unsafe-webgpu')
  app.commandLine.appendSwitch('enable-dawn-features', 'allow_unsafe_apis')
}
app.on('gpu-info-update', () => {
  console.log('GPU Info:', app.getGPUFeatureStatus())
})

const mimeTypeFromExt = (ext: string): string =>
  (
    {
      '.html': 'text/html',
      '.js': 'text/javascript',
      '.css': 'text/css',
      '.png': 'image/png',
      '.jpg': 'image/jpeg',
      '.jpeg': 'image/jpeg',
      '.svg': 'image/svg+xml',
      '.json': 'application/json',
      '.wasm': 'application/wasm',
      '.map': 'application/json',
    } as const
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
;(global as any).carplayService = carplayService

app.on('before-quit', async (e) => {
  if (isQuitting) return
  isQuitting = true
  e.preventDefault()
  try {
    carplayService['shuttingDown'] = true
    await carplayService.stop()
    await usbService['forceReset']?.()
    await usbService.stop()
  } catch (err) {
    console.warn('Error while quitting:', err)
  } finally {
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
      stream: true,
    },
  },
])

const appPath = app.getPath('userData')
const configPath = join(appPath, 'config.json')

const DEFAULT_BINDINGS: KeyBindings = {
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  selectUp: 'KeyB',
  selectDown: 'Space',
  back: 'Backspace',
  home: 'KeyH',
  play: 'KeyP',
  pause: 'KeyO',
  next: 'KeyM',
  prev: 'KeyN',
}

function loadConfig(): ExtraConfig {
  let fileConfig: Partial<ExtraConfig> = {}
  if (existsSync(configPath)) fileConfig = JSON.parse(readFileSync(configPath, 'utf8'))

  const merged: ExtraConfig = {
    ...DEFAULT_CONFIG,
    kiosk: true,
    camera: '',
    microphone: '',
    nightMode: true,
    audioVolume: 1.0,
    navVolume: 0.5,
    audioJitterMs: 15,
    bindings: { ...DEFAULT_BINDINGS },
    ...fileConfig,
  } as ExtraConfig

  merged.bindings = { ...DEFAULT_BINDINGS, ...(fileConfig.bindings || {}) }

  const needWrite = !existsSync(configPath) || JSON.stringify(fileConfig) !== JSON.stringify(merged)
  if (needWrite) {
    writeFileSync(configPath, JSON.stringify(merged, null, 2))
    console.log('[config] Written complete config.json with all defaults')
  }
  return merged
}
config = loadConfig()

// Updater helpers
function pickAssetForPlatform(assets: any[]): { url?: string } {
  if (!Array.isArray(assets)) return {}

  const nameOf = (a: any) => (a?.name || a?.browser_download_url || '') as string
  const urlOf = (a: any) => a?.browser_download_url as string | undefined

  if (process.platform === 'darwin') {
    const dmgs = assets.filter(a => /\.dmg$/i.test(nameOf(a)))
    if (dmgs.length === 0) return {}
    const arch = process.arch
    const preferred =
      arch === 'arm64'
        ? dmgs.find(a => /(arm64|aarch64|apple[-_]?silicon|universal)/i.test(nameOf(a))) ?? dmgs[0]
        : dmgs.find(a => /(x86_64|amd64|x64|universal)/i.test(nameOf(a))) ?? dmgs[0]
    return { url: urlOf(preferred) }
  }

  if (process.platform === 'linux') {
    const appImages = assets.filter(a => /\.AppImage$/i.test(nameOf(a)))
    if (appImages.length === 0) return {}

    let patterns: RegExp[] = []
    if (process.arch === 'x64') {
      patterns = [/[-_.]x86_64\.AppImage$/i, /[-_.]amd64\.AppImage$/i, /[-_.]x64\.AppImage$/i]
    } else if (process.arch === 'arm64') {
      patterns = [/[-_.]arm64\.AppImage$/i, /[-_.]aarch64\.AppImage$/i]
    } else {
      return {}
    }

    const match = appImages.find(a => patterns.some(re => re.test(nameOf(a))))
    return { url: urlOf(match) }
  }

  return {}
}

function sendUpdateEvent(payload: any) {
  mainWindow?.webContents.send('update:event', payload)
}
function sendUpdateProgress(payload: any) {
  mainWindow?.webContents.send('update:progress', payload)
}

async function downloadWithProgress(url: string, dest: string): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const req = https.get(url, (res) => {
      if (res.statusCode && res.statusCode >= 300 && res.headers.location) {
        req.destroy()
        downloadWithProgress(res.headers.location, dest).then(resolve, reject)
        return
      }
      if (res.statusCode !== 200) {
        reject(new Error(`HTTP ${res.statusCode}`))
        return
      }
      const total = parseInt(String(res.headers['content-length'] || 0), 10) || 0
      let received = 0
      const file = createWriteStream(dest)
      res.on('data', (chunk) => {
        received += chunk.length
        sendUpdateProgress({ phase: 'download', received, total, percent: total ? received / total : 0 })
      })
      res.on('error', (err) => reject(err))
      file.on('finish', () => file.close(() => resolve()))
      res.pipe(file)
    })
    req.on('error', reject)
  })
}

// macOS helpers
async function getMacDesiredOwner(dstApp: string): Promise<{ user: string; group: string }> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  if (existsSync(dstApp)) {
    try {
      const out = await new Promise<string>((resolve, reject) =>
        execFile('stat', ['-f', '%Su:%Sg', dstApp], (err, stdout) => (err ? reject(err) : resolve(stdout.trim())))
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
  const appFolder = entries.find((e) => e.isDirectory() && e.name.toLowerCase().endsWith('.app'))?.name
  if (!appFolder) {
    await new Promise<void>((resolve) => execFile('hdiutil', ['detach', mountPoint, '-quiet'], () => resolve()))
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
  await new Promise<void>((resolve, reject) => execFile('osascript', ['-e', script], (err) => (err ? reject(err) : resolve())))

  sendUpdateEvent({ phase: 'unmounting' })
  await new Promise<void>((resolve) => execFile('hdiutil', ['detach', mountPoint, '-quiet'], () => resolve()))
}

// Linux helper
async function installOnLinux(url: string): Promise<void> {
  if (process.platform !== 'linux') throw new Error('Linux only')
  const current = process.env.APPIMAGE
  if (!current) throw new Error('Not running from an AppImage')

  const currentDir = dirname(current)
  const currentBase = basename(current)

  const tmpDownload = join(os.tmpdir(), `pcu-${Date.now()}.AppImage`)
  await downloadWithProgress(url, tmpDownload)

  const tmpRenamed = join(os.tmpdir(), currentBase)
  try {
    if (existsSync(tmpRenamed)) await fsp.unlink(tmpRenamed).catch(() => {})
    await fsp.rename(tmpDownload, tmpRenamed)
  } catch {
    await fsp.copyFile(tmpDownload, tmpRenamed)
    await fsp.unlink(tmpDownload).catch(() => {})
  }

  const destNew = join(currentDir, currentBase + '.new')
  await fsp.copyFile(tmpRenamed, destNew)
  await fsp.chmod(destNew, 0o755)
  await fsp.rename(destNew, current)

  sendUpdateEvent({ phase: 'relaunching' })

  // Relaunch via spawn
  const cleanEnv = { ...process.env }
  delete (cleanEnv as any).APPIMAGE
  delete (cleanEnv as any).APPDIR
  delete (cleanEnv as any).ARGV0
  delete (cleanEnv as any).OWD

  const child = spawn(current, [], { detached: true, stdio: 'ignore', env: cleanEnv })
  child.unref()
  app.exit(0)
}

async function installOnMac(url: string): Promise<void> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  const tmpFile = join(os.tmpdir(), `pcu-${Date.now()}.dmg`)
  await downloadWithProgress(url, tmpFile)
  sendUpdateEvent({ phase: 'installing' })
  await installFromDmg(tmpFile)
  sendUpdateEvent({ phase: 'relaunching' })
  app.relaunch()
  setImmediate(() => app.quit())
}

// Window
function sendKioskSync(kiosk: boolean) {
  mainWindow?.webContents.send('settings:kiosk-sync', kiosk)
}
function persistKioskAndBroadcast(kiosk: boolean) {
  if (config.kiosk === kiosk) return
  config = { ...config, kiosk }
  try {
    writeFileSync(
      configPath,
      JSON.stringify(
        {
          ...config,
          width: +config.width,
          height: +config.height,
          fps: +config.fps,
          dpi: +config.dpi,
          format: +config.format,
          iBoxVersion: +config.iBoxVersion,
          phoneWorkMode: +config.phoneWorkMode,
          packetMax: +config.packetMax,
          mediaDelay: +config.mediaDelay,
          wifiType: config.wifiType,
          wifiChannel: config.wifiChannel,
          primaryColorDark: config.primaryColorDark,
          primaryColorLight: config.primaryColorLight,
        },
        null,
        2
      )
    )
  } catch (e) {
    console.warn('[config] persist kiosk failed:', e)
  }
  if (socket) {
    socket.config = config
    socket.sendSettings()
  }
  sendKioskSync(kiosk)
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
      experimentalFeatures: true,
    },
  })

  const ses = mainWindow.webContents.session
  ses.setPermissionCheckHandler((_w, p) => ['usb', 'hid', 'media', 'display-capture'].includes(p))
  ses.setPermissionRequestHandler((_w, p, cb) => cb(['usb', 'hid', 'media', 'display-capture'].includes(p)))
  ses.setUSBProtectedClassesHandler(({ protectedClasses }) =>
    protectedClasses.filter((c) => ['audio', 'video', 'vendor-specific'].includes(c))
  )

  session.defaultSession.webRequest.onHeadersReceived({ urls: ['*://*/*', 'file://*/*'] }, (d, cb) =>
    cb({
      responseHeaders: {
        ...d.responseHeaders,
        'Cross-Origin-Opener-Policy': ['same-origin'],
        'Cross-Origin-Embedder-Policy': ['require-corp'],
        'Cross-Origin-Resource-Policy': ['same-site'],
      },
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
      if (suppressNextFsSync) { suppressNextFsSync = false; return }
      applyAspectRatioWindowed(mainWindow!, config.width || 800, config.height || 480)
      persistKioskAndBroadcast(false)
    })
  }

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  if (is.dev && process.env.ELECTRON_RENDERER_URL) mainWindow.loadURL(process.env.ELECTRON_RENDERER_URL)
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
      webPreferences: { nodeIntegration: false, contextIsolation: true },
    })
    gpuWindow.loadURL('chrome://gpu')
  }
  if (is.dev) {
    const mediaWindow = new BrowserWindow({
      width: 1000,
      height: 800,
      title: 'GPU Info',
      webPreferences: { nodeIntegration: false, contextIsolation: true },
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
          'Cross-Origin-Resource-Policy': 'same-site',
        },
        data: createReadStream(file),
      })
    } catch (e) {
      console.error('[app-protocol] error', e)
      cb({ statusCode: 500 })
    }
  })

  usbService = new USBService(carplayService)
  socket = new Socket(config, saveSettings)

  ipcMain.handle('quit', () => (isMac
    ? (mainWindow?.isFullScreen()
        ? (() => { suppressNextFsSync = true; mainWindow!.once('leave-full-screen', () => mainWindow?.hide()); mainWindow!.setFullScreen(false) })()
        : mainWindow?.hide())
    : app.quit()))

  ipcMain.handle('settings:get-kiosk', () => currentKiosk())
  ipcMain.handle('getSettings', () => config)
  ipcMain.handle('save-settings', (_evt, settings: ExtraConfig) => {
    saveSettings(settings)
    return true
  })

  ipcMain.handle('app:getVersion', () => app.getVersion())

  ipcMain.handle('app:getLatestRelease', async () => {
    try {
      const repo = process.env.UPDATE_REPO || 'f-io/pi-carplay'
      const feed = process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
      const res = await fetch(feed, { headers: { 'User-Agent': 'pi-carplay-updater' } })
      if (!res.ok) throw new Error(`feed ${res.status}`)
      const json: any = await res.json()
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
      sendUpdateEvent({ phase: 'start' })

      if (process.platform === 'darwin') {
        const url =
          directUrl ||
          (await (async () => {
            const repo = process.env.UPDATE_REPO || 'f-io/pi-carplay'
            const feed = process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
            const res = await fetch(feed, { headers: { 'User-Agent': 'pi-carplay-updater' } })
            const json: any = await res.json()
            return pickAssetForPlatform(json.assets || []).url
          })())
        if (!url || !/\.dmg($|\?)/i.test(url)) throw new Error('No DMG asset found')
        await installOnMac(url)
        return
      } else if (process.platform === 'linux') {
        const url =
          directUrl ||
          (await (async () => {
            const repo = process.env.UPDATE_REPO || 'f-io/pi-carplay'
            const feed = process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
            const res = await fetch(feed, { headers: { 'User-Agent': 'pi-carplay-updater' } })
            const json: any = await res.json()
            return pickAssetForPlatform(json.assets || []).url
          })())
        if (!url || !/\.AppImage($|\?)/i.test(url)) throw new Error('No AppImage asset found')
        await installOnLinux(url)
        return
      }

      console.warn('[update] unsupported platform:', process.platform)
      sendUpdateEvent({ phase: 'error', message: 'Unsupported platform' })
    } catch (e: any) {
      console.warn('[update] failed:', e)
      sendUpdateEvent({ phase: 'error', message: String(e?.message || e) })
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

// Settings IPC
function saveSettings(settings: ExtraConfig) {
  writeFileSync(
    configPath,
    JSON.stringify(
      {
        ...settings,
        width: +settings.width,
        height: +settings.height,
        fps: +settings.fps,
        dpi: +settings.dpi,
        format: +settings.format,
        iBoxVersion: +settings.iBoxVersion,
        phoneWorkMode: +settings.phoneWorkMode,
        packetMax: +settings.packetMax,
        mediaDelay: +settings.mediaDelay,
        wifiType: settings.wifiType,
        wifiChannel: settings.wifiChannel,
        primaryColorDark: settings.primaryColorDark,
        primaryColorLight: settings.primaryColorLight,
      },
      null,
      2
    )
  )

  config = { ...settings }
  socket.config = settings
  socket.sendSettings()
  sendKioskSync(config.kiosk)

  if (!mainWindow) return

  if (isMac) {
    const w = settings.width || 800
    const h = settings.height || 480
    if (settings.kiosk) {
      applyWindowedContentSize(mainWindow, w, h)
      applyAspectRatioFullscreen(mainWindow, w, h)
      mainWindow.setFullScreen(true)
    } else {
      mainWindow.setFullScreen(false)
      applyWindowedContentSize(mainWindow, w, h)
    }
  } else {
    if (settings.kiosk) {
      mainWindow.setKiosk(true)
      applyAspectRatioWindowed(mainWindow, 0, 0)
    } else {
      mainWindow.setKiosk(false)
      mainWindow.setContentSize(settings.width, settings.height, false)
      applyAspectRatioWindowed(mainWindow, settings.width, settings.height)
    }
  }
}
