import { is } from '@electron-toolkit/utils'
import { configEvents, saveSettings } from '@main/ipc/utils'
import { runtimeStateProps } from '@main/types'
import type { Config, WindowBounds } from '@shared/types'
import { BrowserWindow, shell } from 'electron'
import { join } from 'path'

export type SecondaryWindowRole = 'dash' | 'aux'

type SecondaryWindowSpec = {
  role: SecondaryWindowRole
  activeKey: keyof Config
  widthKey: keyof Config
  heightKey: keyof Config
  boundsKey: 'dashScreenBounds' | 'auxScreenBounds'
  title: string
}

const SPECS: SecondaryWindowSpec[] = [
  {
    role: 'dash',
    activeKey: 'dashScreenActive',
    widthKey: 'dashScreenWidth',
    heightKey: 'dashScreenHeight',
    boundsKey: 'dashScreenBounds',
    title: 'Dash'
  },
  {
    role: 'aux',
    activeKey: 'auxScreenActive',
    widthKey: 'auxScreenWidth',
    heightKey: 'auxScreenHeight',
    boundsKey: 'auxScreenBounds',
    title: 'Auxiliary'
  }
]

const windows = new Map<SecondaryWindowRole, BrowserWindow>()
const boundsTimers = new Map<SecondaryWindowRole, NodeJS.Timeout>()

function getSize(cfg: Config, spec: SecondaryWindowSpec) {
  return {
    w: Math.max(1, Number(cfg[spec.widthKey]) || 800),
    h: Math.max(1, Number(cfg[spec.heightKey]) || 480)
  }
}

function getKioskFor(cfg: Config, role: SecondaryWindowRole): boolean {
  return cfg.kiosk?.[role] === true
}

function readBounds(cfg: Config, spec: SecondaryWindowSpec): WindowBounds | undefined {
  const b = cfg[spec.boundsKey]
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

function persistBounds(spec: SecondaryWindowSpec, runtimeState: runtimeStateProps) {
  const win = windows.get(spec.role)
  if (!win || win.isDestroyed()) return
  if (win.isFullScreen() || win.isKiosk()) return
  const [x, y] = win.getPosition()
  const [width, height] = win.getContentSize()
  const next: WindowBounds = { x, y, width, height }
  const prev = runtimeState.config[spec.boundsKey]
  if (
    prev &&
    prev.x === next.x &&
    prev.y === next.y &&
    prev.width === next.width &&
    prev.height === next.height
  ) {
    return
  }
  saveSettings(runtimeState, { [spec.boundsKey]: next } as Partial<Config>)
}

function scheduleBoundsSave(spec: SecondaryWindowSpec, runtimeState: runtimeStateProps) {
  const existing = boundsTimers.get(spec.role)
  if (existing) clearTimeout(existing)
  const t = setTimeout(() => {
    boundsTimers.delete(spec.role)
    persistBounds(spec, runtimeState)
  }, 500)
  boundsTimers.set(spec.role, t)
}

function spawn(spec: SecondaryWindowSpec, runtimeState: runtimeStateProps) {
  const { w, h } = getSize(runtimeState.config, spec)
  const bounds = readBounds(runtimeState.config, spec)
  const wantKiosk = getKioskFor(runtimeState.config, spec.role)

  const win = new BrowserWindow({
    width: bounds?.width ?? w,
    height: bounds?.height ?? h,
    x: bounds?.x,
    y: bounds?.y,
    title: spec.title,
    frame: true,
    useContentSize: true,
    autoHideMenuBar: true,
    backgroundColor: '#000',
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      sandbox: false,
      nodeIntegration: false,
      contextIsolation: true,
      webSecurity: true,
      backgroundThrottling: false
    }
  })

  if (bounds) {
    win.once('ready-to-show', () => {
      if (win.isDestroyed()) return
      win.setBounds({ x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height })
    })
  }

  if (wantKiosk) {
    win.once('ready-to-show', () => {
      if (win.isDestroyed()) return
      if (process.platform === 'darwin') win.setFullScreen(true)
      else win.setKiosk(true)
    })
  }

  win.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  const ses = win.webContents.session
  ses.setPermissionCheckHandler((_w, p) => ['usb', 'hid', 'media', 'display-capture'].includes(p))
  ses.setPermissionRequestHandler((_w, p, cb) =>
    cb(['usb', 'hid', 'media', 'display-capture'].includes(p))
  )

  const url =
    is.dev && process.env.ELECTRON_RENDERER_URL
      ? `${process.env.ELECTRON_RENDERER_URL}?role=${spec.role}`
      : `app://index.html?role=${spec.role}`
  win.loadURL(url)

  const onMoveResize = () => scheduleBoundsSave(spec, runtimeState)
  win.on('move', onMoveResize)
  win.on('resize', onMoveResize)
  win.on('moved', onMoveResize)
  win.on('resized', onMoveResize)
  win.once('ready-to-show', () => scheduleBoundsSave(spec, runtimeState))

  win.on('closed', () => {
    windows.delete(spec.role)
    const t = boundsTimers.get(spec.role)
    if (t) {
      clearTimeout(t)
      boundsTimers.delete(spec.role)
    }
    if (runtimeState.isQuitting) return
    if (runtimeState.config[spec.activeKey] === true) {
      saveSettings(runtimeState, { [spec.activeKey]: false } as Partial<Config>)
    }
  })

  windows.set(spec.role, win)
}

function close(role: SecondaryWindowRole) {
  const win = windows.get(role)
  if (!win) return
  windows.delete(role)
  if (!win.isDestroyed()) win.close()
}

function resize(spec: SecondaryWindowSpec, runtimeState: runtimeStateProps) {
  const win = windows.get(spec.role)
  if (!win || win.isDestroyed()) return
  if (win.isFullScreen() || win.isKiosk()) return
  const { w, h } = getSize(runtimeState.config, spec)
  const [cw, ch] = win.getContentSize()
  if (cw !== w || ch !== h) win.setContentSize(w, h)
}

function applyKiosk(spec: SecondaryWindowSpec, runtimeState: runtimeStateProps) {
  const win = windows.get(spec.role)
  if (!win || win.isDestroyed()) return
  const want = getKioskFor(runtimeState.config, spec.role)
  if (process.platform === 'darwin') {
    if (win.isFullScreen() === want) return
    win.setFullScreen(want)
  } else {
    if (win.isKiosk() === want) return
    win.setKiosk(want)
  }
}

export function syncSecondaryWindows(runtimeState: runtimeStateProps, prev?: Config) {
  if (runtimeState.isQuitting) return
  const cfg = runtimeState.config
  for (const spec of SPECS) {
    const wantActive = cfg[spec.activeKey] === true
    const sizeChanged =
      prev &&
      (prev[spec.widthKey] !== cfg[spec.widthKey] || prev[spec.heightKey] !== cfg[spec.heightKey])
    const kioskChanged = prev && (prev.kiosk?.[spec.role] === true) !== getKioskFor(cfg, spec.role)

    if (wantActive && !windows.has(spec.role)) {
      spawn(spec, runtimeState)
    } else if (!wantActive && windows.has(spec.role)) {
      close(spec.role)
    } else if (wantActive) {
      if (sizeChanged) resize(spec, runtimeState)
      if (kioskChanged) applyKiosk(spec, runtimeState)
    }
  }
}

export function setupSecondaryWindows(runtimeState: runtimeStateProps) {
  syncSecondaryWindows(runtimeState)
  configEvents.on('changed', (next: Config, prev: Config) => {
    void next
    syncSecondaryWindows(runtimeState, prev)
  })
}

export function closeAllSecondaryWindows() {
  for (const role of [...windows.keys()]) close(role)
}

export function getSecondaryWindow(role: SecondaryWindowRole) {
  return windows.get(role) ?? null
}
