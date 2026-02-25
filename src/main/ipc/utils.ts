import { existsSync, writeFileSync } from 'fs'
import { execFile } from 'node:child_process'
import os from 'node:os'
import { runtimeStateProps, UpdateEventPayload } from '@main/types'
import { getMainWindow } from '@main/window/createWindow'
import { DEFAULT_BINDINGS, ExtraConfig } from '@main/Globals'
import { applyNullDeletes, pushSettingsToRenderer, sizesEqual } from '@main/utils'
import {
  applyAspectRatioFullscreen,
  applyAspectRatioWindowed,
  applyWindowedContentSize,
  fitWindowToWorkArea
} from '@main/window/utils'
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

  pushSettingsToRenderer(runtimeState)

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
            runtimeState.config.width || 800,
            runtimeState.config.height || 480
          )
          applyAspectRatioFullscreen(
            mainWindow,
            runtimeState.config.width || 800,
            runtimeState.config.height || 480
          )
        }
        if (!isFs) mainWindow.setFullScreen(true)
      } else {
        if (isFs) mainWindow.setFullScreen(false)
        if (sizeChanged) {
          applyWindowedContentSize(
            mainWindow,
            runtimeState.config.width || 800,
            runtimeState.config.height || 480
          )
        }
      }
    } else if (sizeChanged) {
      if (wantFs) {
        applyWindowedContentSize(
          mainWindow,
          runtimeState.config.width || 800,
          runtimeState.config.height || 480
        )
        applyAspectRatioFullscreen(
          mainWindow,
          runtimeState.config.width || 800,
          runtimeState.config.height || 480
        )
      } else {
        applyWindowedContentSize(
          mainWindow,
          runtimeState.config.width || 800,
          runtimeState.config.height || 480
        )
      }
    }
  } else {
    // Linux
    const win = mainWindow
    if (kioskChanged) {
      const leavingKiosk = !runtimeState.config.kiosk

      // Always drop constraints before switching mode
      applyAspectRatioWindowed(win, 0, 0)

      win.setKiosk(runtimeState.config.kiosk)

      if (leavingKiosk) {
        applyWindowedContentSize(win, runtimeState.config.width, runtimeState.config.height)

        // Re-apply bounds once the WM finishes the transition
        const onResize = () => {
          win.removeListener('resize', onResize)
          fitWindowToWorkArea(win)
          applyWindowedContentSize(win, runtimeState.config.width, runtimeState.config.height)
        }
        win.on('resize', onResize)

        setImmediate(() => {
          if (win.isDestroyed()) return
          fitWindowToWorkArea(win)
          applyWindowedContentSize(win, runtimeState.config.width, runtimeState.config.height)
        })
      } else {
        // entering kiosk
        applyAspectRatioWindowed(win, 0, 0)
      }
      return
    }
    // no kiosk change, only size change in windowed mode
    if (sizeChanged && !runtimeState.config.kiosk) {
      applyWindowedContentSize(win, runtimeState.config.width, runtimeState.config.height)
    }
  }
}
