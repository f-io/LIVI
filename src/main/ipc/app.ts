import { app, ipcMain } from 'electron'
import { getMainWindow } from '@main/window/createWindow'
import { isMacPlatform } from '@main/utils'
import { runtimeStateProps } from '@main/types'

export function registerAppIpc(runtimeState: runtimeStateProps) {
  const mainWindow = getMainWindow()
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
  ipcMain.handle('app:restartApp', () => {
    if (runtimeState.isQuitting) return
    app.relaunch()
    app.quit()
  })
}
