import { registerIpcHandle } from '@main/ipc/register'
import { ServicesProps } from '@main/types'
import { Updater } from './update/updater'

export function registerUpdateIpc(services: ServicesProps) {
  const updater = new Updater(services)
  registerIpcHandle('app:performUpdate', updater.perform)
  registerIpcHandle('app:abortUpdate', updater.abort)
  registerIpcHandle('app:beginInstall', updater.install)
}
