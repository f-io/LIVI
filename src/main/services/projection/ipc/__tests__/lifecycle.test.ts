type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: jest.fn()
}))

import { registerLifecycleIpc } from '../lifecycle'

function freshHost() {
  return {
    start: jest.fn(async () => undefined),
    stop: jest.fn(async () => undefined),
    pickPreferredTransport: jest.fn(() => 'dongle' as 'dongle' | 'aa' | null),
    applyCodecCapabilities: jest.fn()
  }
}

beforeEach(() => {
  handlers.clear()
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('lifecycle ipc', () => {
  test('projection-start delegates to host.start', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-start')!(null)
    expect(host.start).toHaveBeenCalled()
  })

  test('projection-stop refuses to stop when preferred transport is AA', async () => {
    const host = freshHost()
    host.pickPreferredTransport.mockReturnValue('aa')
    registerLifecycleIpc(host)
    await handlers.get('projection-stop')!(null)
    expect(host.stop).not.toHaveBeenCalled()
  })

  test('projection-stop delegates when preferred transport is dongle', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-stop')!(null)
    expect(host.stop).toHaveBeenCalled()
  })

  test('projection-restart calls stop then start', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-restart')!(null)
    expect(host.stop).toHaveBeenCalled()
    expect(host.start).toHaveBeenCalled()
  })

  test('projection-restart swallows a thrown stop()', async () => {
    const host = freshHost()
    host.stop.mockRejectedValueOnce(new Error('not running'))
    registerLifecycleIpc(host)
    await expect(handlers.get('projection-restart')!(null)).resolves.toBeUndefined()
    expect(host.start).toHaveBeenCalled()
  })

  test('projection-codec-capabilities forwards payload', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-codec-capabilities')!(null, { h264: true })
    expect(host.applyCodecCapabilities).toHaveBeenCalledWith({ h264: true })
  })
})
