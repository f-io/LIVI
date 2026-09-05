type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: vi.fn()
}))

import { registerLifecycleIpc } from '../lifecycle'

function freshHost() {
  return {
    start: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
    restartSession: vi.fn(async () => undefined),
    pickPreferredTransport: vi.fn(() => 'aa' as 'aa' | 'cp' | null),
    applyCodecCapabilities: vi.fn(),
    setVideoVisible: vi.fn()
  }
}

beforeEach(async () => {
  handlers.clear()
  vi.spyOn(console, 'warn').mockImplementation(function () {})
})
afterEach(async () => vi.restoreAllMocks())

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

  test('projection-stop delegates when no AA transport is preferred', async () => {
    const host = freshHost()
    host.pickPreferredTransport.mockReturnValue(null)
    registerLifecycleIpc(host)
    await handlers.get('projection-stop')!(null)
    expect(host.stop).toHaveBeenCalled()
  })

  test('projection-restart delegates to host.restartSession', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-restart')!(null)
    expect(host.restartSession).toHaveBeenCalled()
  })

  test('projection-set-visible coerces the flag and forwards it', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-set-visible')!(null, true)
    expect(host.setVideoVisible).toHaveBeenCalledWith(true)
    await handlers.get('projection-set-visible')!(null, undefined)
    expect(host.setVideoVisible).toHaveBeenLastCalledWith(false)
  })

  test('projection-codec-capabilities forwards payload', async () => {
    const host = freshHost()
    registerLifecycleIpc(host)
    await handlers.get('projection-codec-capabilities')!(null, { h264: true })
    expect(host.applyCodecCapabilities).toHaveBeenCalledWith({ h264: true })
  })
})
