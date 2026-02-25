import { app, ipcMain } from 'electron'
import { getMainWindow } from '@main/window/createWindow'
import { isMacPlatform } from '@main/utils'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { restoreKioskAfterWmExit } from '@main/window/utils'
import { spawn } from 'child_process'

export function registerAppIpc(runtimeState: runtimeStateProps, services: ServicesProps) {
  const mainWindow = getMainWindow()
  const { usbService } = services
  const isMac = isMacPlatform()

  ipcMain.handle('quit', () =>
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

  // App Quit
  ipcMain.handle('app:quitApp', () => {
    if (runtimeState.isQuitting) return
    app.quit()
  })

  // App Restart
  ipcMain.handle('app:restartApp', async () => {
    if (runtimeState.isQuitting) return
    runtimeState.isQuitting = true

    try {
      usbService?.beginShutdown()
    } catch {}

    try {
      await usbService?.gracefulReset()
    } catch (e) {
      console.warn('[MAIN] gracefulReset failed (continuing restart):', e)
    }

    await new Promise((r) => setTimeout(r, 150))

    if (process.platform === 'linux' && process.env.APPIMAGE) {
      const appImage = process.env.APPIMAGE

      const cleanEnv = { ...process.env }
      delete cleanEnv.APPIMAGE
      delete cleanEnv.APPDIR
      delete cleanEnv.ARGV0
      delete cleanEnv.OWD

      spawn(appImage, [], { detached: true, stdio: 'ignore', env: cleanEnv }).unref()

      app.exit(0)
      return
    }

    app.relaunch()
    app.exit(0)
  })

  // User activity (touch/click)
  ipcMain.on('app:user-activity', () => {
    restoreKioskAfterWmExit(runtimeState)
  })
}
