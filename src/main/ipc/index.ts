import { registerAppIpc } from '@main/ipc/app'
import { registerSettingsIpc } from '@main/ipc/settings'
import { registerUpdateIpc } from '@main/ipc/update'
import { runtimeStateProps, ServicesProps } from '@main/types'

export function registerIpc(runtimeState: runtimeStateProps, services: ServicesProps) {
  registerAppIpc(runtimeState, services)
  registerSettingsIpc(runtimeState)
  registerUpdateIpc(services)
}
