import { ipcMain } from 'electron'
import { Updater } from './update/updater'

export function registerUpdateIpc() {
  const updater = new Updater()
  ipcMain.handle('app:performUpdate', updater.perform)
  ipcMain.handle('app:abortUpdate', updater.abort)
  ipcMain.handle('app:beginInstall', updater.install)
}
