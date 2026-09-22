import type { Mocked } from 'vitest'

type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: vi.fn()
}))

import { registerBluetoothIpc } from '../bluetooth'
import type { ProjectionIpcHost } from '../types'

type BtHost = Pick<ProjectionIpcHost, 'isStarted' | 'isUsingAa' | 'connectBt' | 'refreshBtPaired'>

function fakeHost(over: Partial<BtHost> = {}): Mocked<BtHost> {
  return {
    isStarted: vi.fn(() => true),
    isUsingAa: vi.fn(() => false),
    connectBt: vi.fn(async () => ({ ok: true })),
    refreshBtPaired: vi.fn(),
    ...over
  } as Mocked<BtHost>
}

beforeEach(() => {
  handlers.clear()
})

describe('bluetooth ipc — projection-bt-pairedlist-set', () => {
  test('returns { ok: false } when not started', async () => {
    registerBluetoothIpc(fakeHost({ isStarted: vi.fn(() => false) }))
    expect(await handlers.get('projection-bt-pairedlist-set')!(null, 'x')).toEqual({ ok: false })
  })

  test('returns { ok: true } when started', async () => {
    registerBluetoothIpc(fakeHost())
    expect(await handlers.get('projection-bt-pairedlist-set')!(null, 'x')).toEqual({ ok: true })
  })
})

describe('bluetooth ipc — projection-bt-connect-device', () => {
  test('rejects when not started or mac empty', async () => {
    registerBluetoothIpc(fakeHost({ isStarted: vi.fn(() => false) }))
    expect(await handlers.get('projection-bt-connect-device')!(null, 'AA')).toEqual({ ok: false })

    registerBluetoothIpc(fakeHost())
    expect(await handlers.get('projection-bt-connect-device')!(null, '  ')).toEqual({ ok: false })
  })

  test('AA path delegates to connectBt and refreshes on success', async () => {
    const host = fakeHost({ isUsingAa: vi.fn(() => true) })
    registerBluetoothIpc(host)
    const res = await handlers.get('projection-bt-connect-device')!(null, 'AA:BB')
    expect(host.connectBt).toHaveBeenCalledWith('AA:BB')
    expect(host.refreshBtPaired).toHaveBeenCalled()
    expect(res).toEqual({ ok: true })
  })

  test('non-AA path returns { ok: false }', async () => {
    const host = fakeHost({ isUsingAa: vi.fn(() => false) })
    registerBluetoothIpc(host)
    const res = (await handlers.get('projection-bt-connect-device')!(null, 'AA:BB')) as {
      ok: boolean
    }
    expect(res.ok).toBe(false)
    expect(host.connectBt).not.toHaveBeenCalled()
  })

  test('AA path: connectBt throwing surfaces as { ok: false, error }', async () => {
    const host = fakeHost({
      isUsingAa: vi.fn(() => true),
      connectBt: vi.fn(async () => {
        throw new Error('boom')
      })
    })
    registerBluetoothIpc(host)
    expect(await handlers.get('projection-bt-connect-device')!(null, 'AA:BB')).toEqual({
      ok: false,
      error: 'boom'
    })
  })

  test('AA path: a rejected connect returns the response without refreshing', async () => {
    const host = fakeHost({
      isUsingAa: vi.fn(() => true),
      connectBt: vi.fn(async () => ({ ok: false }))
    })
    registerBluetoothIpc(host)
    expect(await handlers.get('projection-bt-connect-device')!(null, 'AA:BB')).toEqual({
      ok: false
    })
    expect(host.refreshBtPaired).not.toHaveBeenCalled()
  })

  test('a missing mac is treated as empty', async () => {
    registerBluetoothIpc(fakeHost())
    expect(await handlers.get('projection-bt-connect-device')!(null, undefined)).toEqual({
      ok: false
    })
  })
})

describe('bluetooth ipc — projection-bt-forget-device', () => {
  test('rejects when not started or mac empty', async () => {
    registerBluetoothIpc(fakeHost({ isStarted: vi.fn(() => false) }))
    expect(await handlers.get('projection-bt-forget-device')!(null, 'AA')).toEqual({ ok: false })
  })

  test('forget is not available without a dongle', async () => {
    registerBluetoothIpc(fakeHost())
    const res = (await handlers.get('projection-bt-forget-device')!(null, 'AA:BB')) as {
      ok: boolean
    }
    expect(res.ok).toBe(false)
  })

  test('forget with a missing mac rejects', async () => {
    registerBluetoothIpc(fakeHost())
    expect(await handlers.get('projection-bt-forget-device')!(null, undefined)).toEqual({
      ok: false
    })
  })
})
