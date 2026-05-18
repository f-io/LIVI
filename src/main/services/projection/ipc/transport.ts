import { registerIpcHandle } from '@main/ipc/register'
import type { ProjectionIpcHost } from './types'

type Deps = Pick<ProjectionIpcHost, 'switchTransport' | 'getTransportState'>

export function registerTransportIpc(host: Deps): void {
  registerIpcHandle('transport:switch', async () => host.switchTransport())
  registerIpcHandle('transport:state', async () => host.getTransportState())
}
