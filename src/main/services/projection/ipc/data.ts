import { registerIpcHandle } from '@main/ipc/register'
import type { ProjectionIpcHost } from './types'

export function registerDataIpc(host: ProjectionIpcHost): void {
  registerIpcHandle('projection-media-read', async () => host.readActiveMedia())
  registerIpcHandle('projection-navigation-read', async () => host.readActiveNav())
}
