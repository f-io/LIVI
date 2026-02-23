import { MIN_WIDTH } from '@main/constants'
import { isMacPlatform } from '@main/utils'
import { BrowserWindow } from 'electron'
import { ExtraConfig } from '@main/Globals'
import { getMainWindow } from '@main/window/createWindow'
import { saveSettings } from '@main/ipc/utils'
import { runtimeStateProps } from '@main/types'

export function applyAspectRatioFullscreen(
  win: BrowserWindow,
  width: number,
  height: number
): void {
  const ratio = width && height ? width / height : 0
  win.setAspectRatio(ratio, { width: 0, height: 0 })
}

export function applyAspectRatioWindowed(win: BrowserWindow, width: number, height: number): void {
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

export function applyWindowedContentSize(win: BrowserWindow, w: number, h: number) {
  win.setContentSize(w, h, false)
  applyAspectRatioWindowed(win, w, h)
}

export function currentKiosk(config: ExtraConfig): boolean {
  const mainWindow: BrowserWindow | null = getMainWindow()
  const isMac = isMacPlatform()
  const win = mainWindow
  if (win && !win.isDestroyed()) {
    return isMac ? win.isFullScreen() : win.isKiosk()
  }
  return config.kiosk
}

export function persistKioskAndBroadcast(kiosk: boolean, runtimeState: runtimeStateProps) {
  if (runtimeState.config.kiosk === kiosk) return
  saveSettings(runtimeState, { kiosk })
}

export function sendKioskSync(kiosk: boolean, mainWindow: BrowserWindow | null = null) {
  mainWindow?.webContents.send('settings:kiosk-sync', kiosk)
}
