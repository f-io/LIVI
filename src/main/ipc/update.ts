import { registerIpcHandle } from '@main/ipc/register'
import { runtimeStateProps, ServicesProps } from '@main/types'
import { Updater } from './update/updater'

export function registerUpdateIpc(runtimeState: runtimeStateProps, services: ServicesProps) {
  const updater = new Updater(runtimeState, services)
  registerIpcHandle('app:performUpdate', updater.perform)
  registerIpcHandle('app:abortUpdate', updater.abort)
  registerIpcHandle('app:beginInstall', updater.install)
}
