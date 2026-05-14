import { registerIpcHandle } from '@main/ipc/register'
import { isClusterDisplayed } from '@shared/utils'
import { SendCommand } from '../messages/sendable'
import type { ProjectionIpcHost } from './types'

type Deps = Pick<
  ProjectionIpcHost,
  | 'getConfig'
  | 'setClusterRequested'
  | 'resetLastClusterVideoSize'
  | 'getLastClusterCodec'
  | 'getClusterTargetWebContents'
  | 'send'
>

export function registerClusterIpc(host: Deps): void {
  registerIpcHandle('cluster:request', async (_evt, enabled: boolean) => {
    const allowed = Boolean(enabled) && isClusterDisplayed(host.getConfig())
    host.setClusterRequested(allowed)

    if (!allowed) {
      host.resetLastClusterVideoSize()
      return { ok: true, enabled: false }
    }

    const codec = host.getLastClusterCodec()
    if (codec) {
      for (const wc of host.getClusterTargetWebContents()) {
        try {
          wc.send('projection-event', { type: 'cluster-video-codec', payload: { codec } })
        } catch {
          /* detached webContents */
        }
      }
    }

    try {
      host.send(new SendCommand('requestClusterStreamFocus'))
    } catch {
      // ignore
    }

    return { ok: true, enabled: true }
  })
}
