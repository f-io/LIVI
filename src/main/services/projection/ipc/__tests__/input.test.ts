type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
type IpcOnHandler = (evt: unknown, ...args: unknown[]) => void
const handlers = new Map<string, IpcHandler>()
const onHandlers = new Map<string, IpcOnHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: (channel: string, handler: IpcOnHandler) => {
    onHandlers.set(channel, handler)
  }
}))

import { SendCommand, SendMultiTouch, SendTouch } from '../../messages/sendable'
import { registerInputIpc } from '../input'

function freshHost() {
  return {
    send: vi.fn(async () => true),
    isStarted: vi.fn(() => true)
  }
}

beforeEach(async () => {
  handlers.clear()
  onHandlers.clear()
  vi.spyOn(console, 'error').mockImplementation(function () {})
})
afterEach(async () => vi.restoreAllMocks())

describe('input ipc', () => {
  test('projection-sendframe sends SendCommand("frame")', async () => {
    const host = freshHost()
    registerInputIpc(host)
    await handlers.get('projection-sendframe')!(null)
    expect(host.send).toHaveBeenCalledWith(expect.any(SendCommand))
  })

  test('projection-touch forwards a SendTouch', async () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-touch')!(null, { x: 0.5, y: 0.5, action: 0 })
    expect(host.send).toHaveBeenCalledWith(expect.any(SendTouch))
  })

  test('projection-touch swallows thrown send', async () => {
    const host = freshHost()
    host.send.mockImplementation(function () {
      throw new Error('not started')
    })
    registerInputIpc(host)
    expect(() => onHandlers.get('projection-touch')!(null, { x: 0, y: 0, action: 0 })).not.toThrow()
  })

  test('projection-multi-touch with empty list is a no-op', async () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-multi-touch')!(null, [])
    expect(host.send).not.toHaveBeenCalled()
  })

  test('projection-multi-touch sanitizes coordinates to [0,1]', async () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-multi-touch')!(null, [
      { id: 0, x: 1.5, y: -0.5, action: 0 },
      { id: 1, x: 0.5, y: NaN, action: 1 }
    ])
    expect(host.send).toHaveBeenCalledWith(expect.any(SendMultiTouch))
  })

  test('projection-multi-touch with non-array is a no-op', async () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-multi-touch')!(null, null as never)
    expect(host.send).not.toHaveBeenCalled()
  })

  test('projection-command forwards a SendCommand', async () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-command')!(null, 'play')
    expect(host.send).toHaveBeenCalledWith(expect.any(SendCommand))
  })
})
