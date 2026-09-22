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
      active: 'cp' as const,
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

  test('device handlers delegate to the host', async () => {
    const devices = [{ id: 'AA:BB' }]
    const host = {
      switchTransport: vi.fn(),
      getTransportState: vi.fn(),
      getDevices: vi.fn(() => devices),
      selectDevice: vi.fn(async () => ({ ok: true })),
      cycleSession: vi.fn(async () => ({ ok: true })),
      forgetDevice: vi.fn(async () => ({ ok: true }))
    }
    registerTransportIpc(host)

    await expect(handlers.get('devices:list')!(null)).resolves.toBe(devices)
    await expect(handlers.get('devices:select')!(null, 'AA:BB')).resolves.toEqual({ ok: true })
    expect(host.selectDevice).toHaveBeenCalledWith('AA:BB')
    await expect(handlers.get('devices:cycle')!(null)).resolves.toEqual({ ok: true })
    expect(host.cycleSession).toHaveBeenCalled()
    await expect(handlers.get('devices:forget')!(null, 'AA:BB')).resolves.toEqual({ ok: true })
    expect(host.forgetDevice).toHaveBeenCalledWith('AA:BB')
  })
})
