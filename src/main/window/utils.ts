import { MIN_HEIGHT, MIN_WIDTH } from '@main/constants'
import { saveConfig } from '@main/core'
import { runtimeStateProps } from '@main/types'
import { getMainWindow } from '@main/window/createWindow'
import type { Config, WindowBounds } from '@shared/types'
import { BrowserWindow, screen } from 'electron'

export function applyAspectRatioFullscreen(
  win: BrowserWindow,
  width: number,
  height: number
): void {
  const ratio = width && height ? width / height : 0
  win.setAspectRatio(ratio, { width: 0, height: 0 })
}

export function applyAspectRatioWindowed(win: BrowserWindow, width: number, height: number): void {
  if (!width || !height) {
    win.setAspectRatio(0)
    win.setMinimumSize(0, 0)
    return
  }
  const [winW, winH] = win.getSize()
  const [contentW, contentH] = win.getContentSize()
  const extraW = Math.max(0, winW - contentW)
  const extraH = Math.max(0, winH - contentH)

  win.setAspectRatio(width / height, { width: 0, height: 0 })
  win.setMinimumSize(MIN_WIDTH + extraW, MIN_HEIGHT + extraH)
}

type Size = { width: number; height: number }

/** `want`'s width in the shape of `shape`, shrunk as a whole until it fits `room`. */
export function fitToShape(want: Size, shape: Size, room: Size): Size {
  const at = (v: number) => Math.max(1, Math.round(v))
  if (!(shape.width > 0 && shape.height > 0))
    return { width: at(want.width), height: at(want.height) }
  const width = want.width > 0 ? want.width : shape.width
  const height = (width * shape.height) / shape.width
  const scale = Math.min(
    1,
    room.width > 0 ? room.width / width : 1,
    room.height > 0 ? room.height / height : 1
  )
  return { width: at(width * scale), height: at(height * scale) }
}

function contentRoom(win: BrowserWindow): Size {
  const work = screen.getDisplayMatching(win.getBounds()).workAreaSize
  const [winW, winH] = win.getSize()
  const [contentW, contentH] = win.getContentSize()
  return {
    width: work.width - Math.max(0, winW - contentW),
    height: work.height - Math.max(0, winH - contentH)
  }
}

/** Fitted into the screen with the shape of `w` x `h` kept, else the system cuts it into another. */
export function applyWindowedContentSize(
  win: BrowserWindow,
  w: number,
  h: number,
  want: Size = { width: w, height: h }
) {
  if (process.platform === 'linux') {
    win.setResizable(true)
    win.setMinimumSize(0, 0)
  }
  const size = fitToShape(want, { width: w, height: h }, contentRoom(win))
  win.setContentSize(size.width, size.height, false)
  applyAspectRatioWindowed(win, w, h)
}

// Saved bounds can point at a monitor that is unplugged or moved.
export function sanitizeBounds(b?: WindowBounds): WindowBounds | undefined {
  if (
    !b ||
    typeof b.x !== 'number' ||
    typeof b.y !== 'number' ||
    typeof b.width !== 'number' ||
    typeof b.height !== 'number' ||
    b.width < 1 ||
    b.height < 1
  ) {
    return undefined
  }
  const MIN_VISIBLE = 64
  const displays = typeof screen?.getAllDisplays === 'function' ? screen.getAllDisplays() : []
  for (const d of displays) {
    const wa = d.workArea
    const overlapW = Math.min(b.x + b.width, wa.x + wa.width) - Math.max(b.x, wa.x)
    const overlapH = Math.min(b.y + b.height, wa.y + wa.height) - Math.max(b.y, wa.y)
    if (overlapW >= MIN_VISIBLE && overlapH >= MIN_VISIBLE) return b
  }
  return displays.length === 0 ? b : undefined
}

export function getMainKiosk(config: Config): boolean {
  return config.kiosk?.main === true
}

export function withMainKiosk(config: Config, value: boolean): Config['kiosk'] {
  const prev = config.kiosk ?? { main: false, cluster: false, aux: false }
  return { ...prev, main: value }
}

export function persistKiosk(kiosk: boolean, runtimeState: runtimeStateProps) {
  if (getMainKiosk(runtimeState.config) === kiosk) return
  runtimeState.wmExitedKiosk = false
  saveConfig({ kiosk: withMainKiosk(runtimeState.config, kiosk) })
}

export function restoreKioskAfterWmExit(runtimeState: runtimeStateProps) {
  const mainWindow: BrowserWindow | null = getMainWindow()

  if (process.platform !== 'linux') return
  if (!mainWindow || mainWindow.isDestroyed()) return
  if (!runtimeState.wmExitedKiosk) return

  runtimeState.wmExitedKiosk = false

  try {
    mainWindow.setKiosk(true)
  } catch {}

  saveConfig({ kiosk: withMainKiosk(runtimeState.config, true) })
}

export function attachKioskStateSync(runtimeState: runtimeStateProps) {
  const win: BrowserWindow | null = getMainWindow()

  if (process.platform !== 'linux') return
  if (!win) return

  let lastSent: boolean | null = null

  const push = (effectiveKiosk: boolean) => {
    if (lastSent === effectiveKiosk) return
    lastSent = effectiveKiosk

    if (effectiveKiosk === getMainKiosk(runtimeState.config)) return

    // Leaving kiosk here is a WM swipe, the next focus restores kiosk. Entering from the
    // compositor titlebar button is only saved.
    if (!effectiveKiosk) {
      runtimeState.wmExitedKiosk = true
    }
    saveConfig({ kiosk: withMainKiosk(runtimeState.config, effectiveKiosk) })
  }

  const syncFromElectron = () => {
    if (win.isDestroyed()) return
    push(process.env.LIVI_COMPOSITOR === '1' ? win.isFullScreen() : win.isKiosk())
  }

  win.on('enter-full-screen', syncFromElectron)
  win.on('leave-full-screen', syncFromElectron)
  win.on('resize', syncFromElectron)
  win.on('move', syncFromElectron)
  win.on('show', syncFromElectron)

  win.on('focus', () => {
    restoreKioskAfterWmExit(runtimeState)
  })

  win.on('blur', syncFromElectron)
  win.on('restore', syncFromElectron)
  win.on('minimize', syncFromElectron)

  syncFromElectron()
}

// In the compositor Chromium keeps the old size until it sees one more resize.
export function attachResizeReflow() {
  if (process.env.LIVI_COMPOSITOR !== '1') return
  const win: BrowserWindow | null = getMainWindow()
  if (!win) return

  let timer: ReturnType<typeof setTimeout> | null = null
  let nudging = false

  win.on('resize', () => {
    if (nudging || win.isDestroyed()) return
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      if (win.isDestroyed() || win.isFullScreen()) return
      nudging = true
      const [w, h] = win.getContentSize()
      win.setContentSize(w, h + 1)
      setTimeout(() => {
        if (!win.isDestroyed()) win.setContentSize(w, h)
        setTimeout(() => {
          nudging = false
        }, 60)
      }, 60)
    }, 200)
  })
}

export const MIN_UI_VIEWPORT_HEIGHT = 292

export function uiZoomFactor(uiZoomPercent: number | undefined, contentHeight: number): number {
  const base = contentHeight > 0 ? Math.min(1, contentHeight / MIN_UI_VIEWPORT_HEIGHT) : 1
  return ((uiZoomPercent ?? 100) / 100) * base
}
