import { ipcMain } from 'electron'
import { Updater } from './update/updater'
import { ServicesProps } from '@main/types'

export function registerUpdateIpc(services: ServicesProps) {
  const updater = new Updater(services)
  ipcMain.handle('app:performUpdate', updater.perform)
  ipcMain.handle('app:abortUpdate', updater.abort)
  ipcMain.handle('app:beginInstall', updater.install)
}
