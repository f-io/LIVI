import { registerAppIpc } from '@main/ipc/app'
import { registerSettingsIpc } from '@main/ipc/settings'
import { registerUpdateIpc } from '@main/ipc/update'
import { runtimeStateProps } from '@main/types'

export function registerIpc(runtimeState: runtimeStateProps) {
  registerAppIpc(runtimeState)
  registerSettingsIpc(runtimeState)
  registerUpdateIpc()
}
