import { existsSync, writeFileSync } from 'fs'
import { execFile } from 'node:child_process'
import os from 'node:os'
import { runtimeStateProps, UpdateEventPayload } from '@main/types'
import { getMainWindow } from '@main/window/createWindow'
import { DEFAULT_BINDINGS, ExtraConfig } from '@main/Globals'
import { applyNullDeletes, sizesEqual } from '@main/utils'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  sendKioskSync
} from '@main/window/utils'
import { DEFAULT_HEIGHT, DEFAULT_WIDTH } from '@main/constants'
import { CONFIG_PATH } from '@main/config/paths'

export async function getMacDesiredOwner(dstApp: string): Promise<{ user: string; group: string }> {
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

export function sendUpdateEvent(payload: UpdateEventPayload) {
  const mainWindow = getMainWindow()
  mainWindow?.webContents.send('update:event', payload)
}

export function sendUpdateProgress(payload: Extract<UpdateEventPayload, { phase: 'download' }>) {
  const mainWindow = getMainWindow()
  mainWindow?.webContents.send('update:progress', payload)
}

export function saveSettings(runtimeState: runtimeStateProps, next: Partial<ExtraConfig>) {
  const mainWindow = getMainWindow()
  const merged: ExtraConfig = {
    ...runtimeState.config,
    ...next,
    bindings: {
      ...DEFAULT_BINDINGS,
      ...(runtimeState.config.bindings ?? {}),
      ...(next.bindings ?? {})
    }
  } as ExtraConfig

  applyNullDeletes(merged, next)

  try {
    writeFileSync(CONFIG_PATH, JSON.stringify(merged, null, 2))
  } catch (e) {
    console.warn('[config] saveSettings failed:', e)
  }

  const prev = runtimeState.config
  runtimeState.config = merged

  if (runtimeState.socket) {
    runtimeState.socket.config = runtimeState.config
    runtimeState.socket.sendSettings()
  }
  sendKioskSync(runtimeState.config.kiosk)

  if (!mainWindow) return

  const sizeChanged = !sizesEqual(prev, runtimeState.config)
  const kioskChanged = prev.kiosk !== runtimeState.config.kiosk

  if (process.platform === 'darwin') {
    const wantFs = runtimeState.config.kiosk
    const isFs = mainWindow.isFullScreen()

    if (kioskChanged) {
      if (wantFs) {
        if (sizeChanged) {
          applyWindowedContentSize(
            mainWindow,
            runtimeState.config.width || DEFAULT_WIDTH,
            runtimeState.config.height || DEFAULT_HEIGHT
          )
          applyAspectRatioFullscreen(
            mainWindow,
            runtimeState.config.width || DEFAULT_WIDTH,
            runtimeState.config.height || DEFAULT_HEIGHT
          )
        }
        if (!isFs) mainWindow.setFullScreen(true)
      } else {
        if (isFs) mainWindow.setFullScreen(false)
        if (sizeChanged) {
          applyWindowedContentSize(
            mainWindow,
            runtimeState.config.width || DEFAULT_WIDTH,
            runtimeState.config.height || DEFAULT_HEIGHT
          )
        }
      }
    } else if (sizeChanged) {
      if (wantFs) {
        applyWindowedContentSize(
          mainWindow,
          runtimeState.config.width || DEFAULT_WIDTH,
          runtimeState.config.height || DEFAULT_HEIGHT
        )
        applyAspectRatioFullscreen(
          mainWindow,
          runtimeState.config.width || DEFAULT_WIDTH,
          runtimeState.config.height || DEFAULT_HEIGHT
        )
      } else {
        applyWindowedContentSize(
          mainWindow,
          runtimeState.config.width || DEFAULT_WIDTH,
          runtimeState.config.height || DEFAULT_HEIGHT
        )
      }
    }
  } else {
    // Linux
    if (kioskChanged) {
      mainWindow.setKiosk(runtimeState.config.kiosk)
      if (sizeChanged) {
        if (runtimeState.config.kiosk) {
          applyAspectRatioWindowed(mainWindow, 0, 0)
        } else {
          mainWindow.setContentSize(runtimeState.config.width, runtimeState.config.height, false)
          applyAspectRatioWindowed(
            mainWindow,
            runtimeState.config.width,
            runtimeState.config.height
          )
        }
      }
    } else if (sizeChanged) {
      if (runtimeState.config.kiosk) {
        applyAspectRatioWindowed(mainWindow, 0, 0)
      } else {
        mainWindow.setContentSize(runtimeState.config.width, runtimeState.config.height, false)
        applyAspectRatioWindowed(mainWindow, runtimeState.config.width, runtimeState.config.height)
      }
    }
  }
}
