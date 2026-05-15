import { registerIpcHandle, registerIpcOn } from '@main/ipc/register'
import { SendCommand, SendMultiTouch, SendRawMessage, SendTouch } from '../messages/sendable'
import type { ProjectionIpcHost } from './types'

type MultiTouchPoint = { id: number; x: number; y: number; action: number }

const ONE_BASED_IDS = false

const to01 = (v: number): number => {
  const n = Number.isFinite(v) ? v : 0
  return n < 0 ? 0 : n > 1 ? 1 : n
}

type Deps = Pick<ProjectionIpcHost, 'send' | 'isStarted'>

export function registerInputIpc(host: Deps): void {
  registerIpcHandle('projection-sendframe', async () => host.send(new SendCommand('frame')))

  registerIpcOn('projection-touch', (_evt, data: { x: number; y: number; action: number }) => {
    try {
      host.send(new SendTouch(data.x, data.y, data.action))
    } catch {
      // ignore
    }
  })

  registerIpcOn('projection-multi-touch', (_evt, points: MultiTouchPoint[]) => {
    try {
      if (!Array.isArray(points) || points.length === 0) return
      const safe = points.map((p) => ({
        id: (p.id | 0) + (ONE_BASED_IDS ? 1 : 0),
        x: to01(p.x),
        y: to01(p.y),
        action: p.action | 0
      }))
      host.send(new SendMultiTouch(safe))
    } catch {
      // ignore
    }
  })

  registerIpcOn('projection-raw-message', (_evt, payload: { type: number; data: number[] }) => {
    try {
      if (!host.isStarted()) return
      host.send(new SendRawMessage(payload.type, new Uint8Array(payload.data ?? [])))
    } catch (e) {
      console.error('[projection-ipc] raw-message failed', e)
    }
  })

  registerIpcOn(
    'projection-command',
    (_evt, command: ConstructorParameters<typeof SendCommand>[0]) => {
      host.send(new SendCommand(command))
    }
  )
}
