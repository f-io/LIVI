import { registerIpcHandle } from '@main/ipc/register'
import type { ProjectionIpcHost } from './types'

type Deps = Pick<ProjectionIpcHost, 'flipTransport' | 'getTransportState'>

export function registerTransportIpc(host: Deps): void {
  registerIpcHandle('transport:flip', async () => host.flipTransport())
  registerIpcHandle('transport:state', async () => host.getTransportState())
}
