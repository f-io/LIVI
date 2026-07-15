type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: vi.fn()
}))

import { registerTransportIpc } from '../transport'

describe('transport ipc', () => {
  beforeEach(async () => handlers.clear())

  test('transport:switch delegates to host.switchTransport', async () => {
    const host = {
      switchTransport: vi.fn(async () => ({ ok: true, active: 'aa' as const })),
      getTransportState: vi.fn()
    }
    registerTransportIpc(host)
    const r = await handlers.get('transport:switch')!(null)
    expect(host.switchTransport).toHaveBeenCalled()
    expect(r).toEqual({ ok: true, active: 'aa' })
  })

  test('transport:state delegates to host.getTransportState', async () => {
    const state = {
      active: 'dongle' as const,
      dongleDetected: true,
      nativeDetected: false
    }
    const host = {
      switchTransport: vi.fn(),
      getTransportState: vi.fn(() => state)
    }
    registerTransportIpc(host)
    const r = await handlers.get('transport:state')!(null)
    expect(r).toBe(state)
  })
})
