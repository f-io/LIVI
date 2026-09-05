import { registerIpcHandle } from '@main/ipc/register'
import type { ProjectionIpcHost } from './types'

type Deps = Pick<ProjectionIpcHost, 'isStarted' | 'isUsingAa' | 'connectBt' | 'refreshBtPaired'>

export function registerBluetoothIpc(host: Deps): void {
  registerIpcHandle('projection-bt-pairedlist-set', async () => {
    return { ok: host.isStarted() }
  })

  registerIpcHandle('projection-bt-connect-device', async (_evt, mac: string) => {
    if (!host.isStarted()) return { ok: false }
    const btMac = String(mac ?? '').trim()
    if (!btMac) return { ok: false }
    if (!host.isUsingAa()) return { ok: false, error: 'no wireless device to connect' }
    try {
      const resp = await host.connectBt(btMac)
      if (resp.ok) host.refreshBtPaired()
      return resp
    } catch (e) {
      return { ok: false, error: (e as Error).message }
    }
  })

  registerIpcHandle('projection-bt-forget-device', async (_evt, mac: string) => {
    if (!host.isStarted() || !String(mac ?? '').trim()) return { ok: false }
    return { ok: false, error: 'forget is not available without a dongle' }
  })
}
