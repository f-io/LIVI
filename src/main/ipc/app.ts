import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import {
  CUSTOM_ICON_URL,
  CUSTOM_PAGE_URL,
  customIconExists,
  customPageExists
} from '@main/protocol/appProtocol'
import { customProxy } from '@main/services/custom/CustomProxy'
import { hostPowerAvailable, requestPowerAction } from '@main/services/power/hostPower'
import { compositorRestart } from '@main/services/video/GstVideo'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { isMacPlatform } from '@main/utils'
import { broadcastToRenderers } from '@main/window/broadcast'
import { getMainWindow } from '@main/window/createWindow'
import { restoreKioskAfterWmExit } from '@main/window/utils'
import { spawn } from 'child_process'
import { app, shell } from 'electron'

let restartInProgress = false

export async function restartApp(
  runtimeState: runtimeStateProps,
  services: ServicesProps
): Promise<void> {
  if (restartInProgress) return
  if (runtimeState.isQuitting) return
  restartInProgress = true

  // Guard the async teardown window only, reset at the end so a prevented quit is not stuck.
  try {
    if (hostPowerAvailable()) {
      requestPowerAction('reboot')
      app.quit()
      return
    }

    try {
      const teardown = services.projectionService.shutdownWirelessSessions()
      await Promise.race([teardown, new Promise((r) => setTimeout(r, 8000))])
    } catch (e) {
      console.warn('[MAIN] shutdownWirelessSessions failed (continuing restart):', e)
    }

    // A restart leaves the phones paired and connected, only the helper goes.
    try {
      await services.projectionService.stopHelper()
    } catch (e) {
      console.warn('[MAIN] stopHelper failed (continuing restart):', e)
    }

    await new Promise((r) => setTimeout(r, 150))

    try {
      await runtimeState.telemetrySocket?.disconnect?.()
    } catch {
      // best-effort
    }

    // In the compositor: tell it to re-exec (full_restart), then quit ourselves cleanly so our
    // surfaces disconnect while the compositor loop is still alive
    if (compositorRestart()) {
      await new Promise((r) => setTimeout(r, 100))
      runtimeState.isQuitting = true
      app.quit()
      return
    }

    if (process.platform === 'linux' && process.env.APPIMAGE) {
      const appImage = process.env.APPIMAGE
      const cleanEnv = { ...process.env }
      delete cleanEnv.APPIMAGE
      delete cleanEnv.APPDIR
      delete cleanEnv.ARGV0
      delete cleanEnv.OWD
      spawn(appImage, [], { detached: true, stdio: 'ignore', env: cleanEnv }).unref()
    } else {
      app.relaunch()
    }

    app.quit()
  } finally {
    restartInProgress = false
  }
}

export function registerAppIpc(runtimeState: runtimeStateProps, services: ServicesProps) {
  const mainWindow = getMainWindow()
  const isMac = isMacPlatform()

  registerIpcHandle('quit', () =>
    isMac
      ? mainWindow?.isFullScreen()
        ? (() => {
            runtimeState.suppressNextFsSync = true
            mainWindow!.once('leave-full-screen', () => mainWindow?.hide())
            mainWindow!.setFullScreen(false)
          })()
        : mainWindow?.hide()
      : app.quit()
  )

  registerIpcHandle('app:customPageUrl', async () => {
    const proxied = await customProxy.start(runtimeState.config.customUrl)
    if (proxied) return proxied
    return customPageExists() ? CUSTOM_PAGE_URL : null
  })

  // App Quit
  registerIpcHandle('app:customIconUrl', () => (customIconExists() ? CUSTOM_ICON_URL : null))

  registerIpcHandle('app:quitApp', () => {
    if (runtimeState.isQuitting) return
    if (hostPowerAvailable()) requestPowerAction('poweroff')
    app.quit()
  })

  // App Restart
  registerIpcHandle('app:restartApp', () => restartApp(runtimeState, services))

  // User activity (touch/click)
  registerIpcOn('ui:path', (_evt, path: string) => {
    services.projectionService.setUiPath(String(path ?? ''))
  })

  registerIpcOn('app:user-activity', () => {
    restoreKioskAfterWmExit(runtimeState)
  })

  // Fan-out a media key event to all renderer windows
  registerIpcOn('app:media-key', (_evt, command: string) => {
    if (typeof command !== 'string' || !command) return
    broadcastToRenderers('app:media-key', command)
  })

  registerIpcHandle('app:openExternal', async (_evt, rawUrl: string) => {
    const url = String(rawUrl ?? '').trim()
    if (!url) return { ok: false, error: 'Empty URL' }
    if (!/^https?:\/\//i.test(url)) return { ok: false, error: 'Only http/https URLs are allowed' }

    await shell.openExternal(url)
    return { ok: true }
  })
}
