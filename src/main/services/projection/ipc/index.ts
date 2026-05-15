import { registerAudioIpc } from './audio'
import { registerBluetoothIpc } from './bluetooth'
import { registerClusterIpc } from './cluster'
import { registerDataIpc } from './data'
import { registerDongleIpc } from './dongle'
import { registerInputIpc } from './input'
import { registerLifecycleIpc } from './lifecycle'
import { registerTransportIpc } from './transport'
import type { ProjectionIpcHost } from './types'

export function registerProjectionIpc(host: ProjectionIpcHost): void {
  registerLifecycleIpc(host)
  registerTransportIpc(host)
  registerInputIpc(host)
  registerBluetoothIpc(host)
  registerClusterIpc(host)
  registerDataIpc()
  registerDongleIpc(host)
  registerAudioIpc(host)
}

export type { ProjectionIpcHost } from './types'
