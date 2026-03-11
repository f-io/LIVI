import { Updater } from './update/updater'
import { ServicesProps } from '@main/types'
import { registerIpcHandle } from '@main/ipc/register'

export function registerUpdateIpc(services: ServicesProps) {
  const updater = new Updater(services)
  registerIpcHandle('app:performUpdate', updater.perform)
  registerIpcHandle('app:abortUpdate', updater.abort)
  registerIpcHandle('app:beginInstall', updater.install)
}
