import { registerIpcHandle } from '@main/ipc/register'
import type { ProjectionIpcHost } from './types'

type Deps = Pick<
  ProjectionIpcHost,
  | 'switchTransport'
  | 'getTransportState'
  | 'getDevices'
  | 'selectDevice'
  | 'cycleSession'
  | 'forgetDevice'
>

export function registerTransportIpc(host: Deps): void {
  registerIpcHandle('transport:switch', async () => host.switchTransport())
  registerIpcHandle('transport:state', async () => host.getTransportState())
  registerIpcHandle('devices:list', async () => host.getDevices())
  registerIpcHandle('devices:select', async (_evt, id: string) => host.selectDevice(id))
  registerIpcHandle('devices:cycle', async () => host.cycleSession())
  registerIpcHandle('devices:forget', async (_evt, id: string) => host.forgetDevice(id))
}
