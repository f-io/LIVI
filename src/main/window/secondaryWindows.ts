import { EventEmitter } from 'node:events'
import { COMPOSITOR_TITLEBAR_H } from '@main/app/compositorLayout'
import {
  DEFAULT_HEIGHT,
  DEFAULT_WIDTH,
  MAX_HEIGHT,
  MAX_WIDTH,
  MIN_HEIGHT,
  MIN_WIDTH
} from '@main/constants'
import { configEvents, saveSettings } from '@main/ipc/utils'
import { backdropHex, setCompositorScreen, setMacBackdrop } from '@main/services/video/GstVideo'
import { runtimeStateProps } from '@main/types'
import { isDev } from '@main/utils'
import type { Config, WindowBounds } from '@shared/types'
import { BrowserWindow, shell } from 'electron'
import { join } from 'path'
import { sanitizeBounds } from './utils'

// Inside livi-compositor the host window is the compositor's own output (titled by role);
// the Electron title only tells the compositor which screen this window belongs to.
const inCompositor = process.env.LIVI_COMPOSITOR === '1'

// Fires 'ready' with the role once a secondary window has loaded, so listeners
// can attach content that needs a live webContents (e.g. cluster planes).
export const secondaryWindowEvents = new EventEmitter()
// one 'ready' listener per ProjectionService instance
secondaryWindowEvents.setMaxListeners(0)

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

// A size out of range reaches this from a hand-edited config as easily as from the
// UI, so the window geometry is bounded here regardless of where the value came from.
function clampSize(v: unknown, fallback: number, min: number, max: number): number {
  const n = Math.round(Number(v))
  if (!Number.isFinite(n) || n <= 0) return fallback
  return Math.min(max, Math.max(min, n))
}

function getSize(cfg: Config, spec: SecondaryWindowSpec) {
  return {
    w: clampSize(cfg[spec.widthKey], DEFAULT_WIDTH, MIN_WIDTH, MAX_WIDTH),
    h: clampSize(cfg[spec.heightKey], DEFAULT_HEIGHT, MIN_HEIGHT, MAX_HEIGHT)
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
  // In the compositor the host window's geometry is WM-managed
  if (inCompositor) return
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
  const bounds = inCompositor ? undefined : sanitizeBounds(readBounds(runtimeState.config, spec))
  const wantKiosk = getKioskFor(runtimeState.config, spec.role)

  const win = new BrowserWindow({
    width: bounds?.width ?? w,
    height: bounds?.height ?? h,
    x: bounds?.x,
    y: bounds?.y,
    title: inCompositor ? `livi:${spec.role}` : spec.title,
    frame: !inCompositor,
    useContentSize: true,
    autoHideMenuBar: true,
    transparent: inCompositor,
    backgroundColor: inCompositor || process.platform === 'darwin' ? '#00000000' : '#000',
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
      win.setContentSize(bounds.width, bounds.height)
      win.setPosition(bounds.x, bounds.y)
    })
  }

  if (wantKiosk) {
    win.once('ready-to-show', () => {
      if (win.isDestroyed()) return
      // In the compositor, fullscreen the HOST output via xdg set_fullscreen
      if (process.platform === 'darwin' || inCompositor) win.setFullScreen(true)
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
    isDev() && process.env.ELECTRON_RENDERER_URL
      ? `${process.env.ELECTRON_RENDERER_URL}?role=${spec.role}`
      : `app://index.html?role=${spec.role}`
  win.loadURL(url)

  const onMoveResize = () => scheduleBoundsSave(spec, runtimeState)
  win.on('move', onMoveResize)
  win.on('resize', onMoveResize)
  win.on('moved', onMoveResize)
  win.on('resized', onMoveResize)
  win.once('ready-to-show', () => scheduleBoundsSave(spec, runtimeState))

  // The window is spawned lazily, the global applyBackdrop loop misses it before its
  // content view is realized. Paint the backdrop once the window is ready.
  win.once('ready-to-show', () => {
    if (win.isDestroyed()) return
    const cfg = runtimeState.config
    setMacBackdrop(
      win,
      backdropHex(cfg.darkMode, cfg.backgroundColorDark, cfg.backgroundColorLight)
    )
  })

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
  win.webContents.once?.('did-finish-load', () => secondaryWindowEvents.emit('ready', spec.role))
}

function close(role: SecondaryWindowRole) {
  const win = windows.get(role) as BrowserWindow
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
  if (process.platform === 'darwin' || inCompositor) {
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

    const { w, h } = getSize(cfg, spec)
    const outH = getKioskFor(cfg, spec.role) ? h : h + COMPOSITOR_TITLEBAR_H

    if (!prev || prev[spec.activeKey] !== cfg[spec.activeKey]) {
      setCompositorScreen(spec.role, wantActive, w, outH)
    }

    if (wantActive && !windows.has(spec.role)) {
      spawn(spec, runtimeState)
    } else if (!wantActive && windows.has(spec.role)) {
      close(spec.role)
    } else if (wantActive) {
      if (sizeChanged) {
        resize(spec, runtimeState)

        if (inCompositor) {
          setCompositorScreen(spec.role, false)
          setCompositorScreen(spec.role, true, w, outH)
        }
      }
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
