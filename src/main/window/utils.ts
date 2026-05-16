import { MIN_HEIGHT, MIN_WIDTH } from '@main/constants'
import { saveSettings } from '@main/ipc/utils'
import { runtimeStateProps } from '@main/types'
import { isMacPlatform, pushSettingsToRenderer } from '@main/utils'
import { getMainWindow } from '@main/window/createWindow'
import type { Config } from '@shared/types'
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

  win.setAspectRatio(0)
  win.setMinimumSize(MIN_WIDTH + extraW, MIN_HEIGHT + extraH)
}

export function applyWindowedContentSize(win: BrowserWindow, w: number, h: number) {
  if (process.platform === 'linux') {
    const d = screen.getDisplayMatching(win.getBounds())
    const work = d.workAreaSize

    const dipW = Math.max(1, Math.min(Math.round(w), work.width))
    const dipH = Math.max(1, Math.min(Math.round(h), work.height))

    win.setResizable(true)
    win.setMinimumSize(0, 0)

    win.setContentSize(dipW, dipH, false)
    applyAspectRatioWindowed(win, dipW, dipH)
    return
  }

  // non-Linux
  win.setContentSize(w, h, false)
  applyAspectRatioWindowed(win, w, h)
}

export function getMainKiosk(config: Config): boolean {
  return config.kiosk?.main === true
}

export function withMainKiosk(config: Config, value: boolean): Config['kiosk'] {
  const prev = config.kiosk ?? { main: false, dash: false, aux: false }
  return { ...prev, main: value }
}

export function currentKiosk(config: Config): boolean {
  const win: BrowserWindow | null = getMainWindow()
  const isMac = isMacPlatform()

  if (win && !win.isDestroyed()) {
    return isMac ? win.isFullScreen() : win.isKiosk()
  }
  return getMainKiosk(config)
}

export function persistKioskAndBroadcast(kiosk: boolean, runtimeState: runtimeStateProps) {
  if (getMainKiosk(runtimeState.config) === kiosk) {
    pushSettingsToRenderer(runtimeState, { kiosk: withMainKiosk(runtimeState.config, kiosk) })
    return
  }

  runtimeState.wmExitedKiosk = false
  saveSettings(runtimeState, { kiosk: withMainKiosk(runtimeState.config, kiosk) })
}

export function sendKioskSync(kiosk: boolean, mainWindow: BrowserWindow | null = null) {
  mainWindow?.webContents.send('settings:kiosk-sync', kiosk)
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

  saveSettings(runtimeState, { kiosk: withMainKiosk(runtimeState.config, true) })
}

export function attachKioskStateSync(runtimeState: runtimeStateProps) {
  const win: BrowserWindow | null = getMainWindow()

  if (process.platform !== 'linux') return
  if (!win) return

  let lastSent: boolean | null = null

  const push = (effectiveKiosk: boolean) => {
    if (lastSent === effectiveKiosk) return
    lastSent = effectiveKiosk

    if (!effectiveKiosk && getMainKiosk(runtimeState.config)) {
      runtimeState.wmExitedKiosk = true
      saveSettings(runtimeState, { kiosk: withMainKiosk(runtimeState.config, false) })
      return
    }

    pushSettingsToRenderer(runtimeState, {
      kiosk: withMainKiosk(runtimeState.config, effectiveKiosk)
    })
  }

  const syncFromElectron = () => {
    if (win.isDestroyed()) return
    push(win.isKiosk())
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
