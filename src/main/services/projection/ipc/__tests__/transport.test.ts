type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: jest.fn()
}))

import { registerTransportIpc } from '../transport'

describe('transport ipc', () => {
  beforeEach(() => handlers.clear())

  test('transport:flip delegates to host.flipTransport', async () => {
    const host = {
      flipTransport: jest.fn(async () => ({ ok: true, active: 'aa' as const })),
      getTransportState: jest.fn()
    }
    registerTransportIpc(host)
    const r = await handlers.get('transport:flip')!(null)
    expect(host.flipTransport).toHaveBeenCalled()
    expect(r).toEqual({ ok: true, active: 'aa' })
  })

  test('transport:state delegates to host.getTransportState', async () => {
    const state = {
      active: 'dongle' as const,
      dongleDetected: true,
      nativeDetected: false,
      preference: 'auto' as const
    }
    const host = {
      flipTransport: jest.fn(),
      getTransportState: jest.fn(() => state)
    }
    registerTransportIpc(host)
    const r = await handlers.get('transport:state')!(null)
    expect(r).toBe(state)
  })
})
