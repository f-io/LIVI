type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
type IpcOnHandler = (evt: unknown, ...args: unknown[]) => void
const handlers = new Map<string, IpcHandler>()
const onHandlers = new Map<string, IpcOnHandler>()

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: (channel: string, handler: IpcOnHandler) => {
    onHandlers.set(channel, handler)
  }
}))

import { SendCommand, SendMultiTouch, SendRawMessage, SendTouch } from '../../messages/sendable'
import { registerInputIpc } from '../input'

function freshHost() {
  return {
    send: jest.fn(async () => true),
    isStarted: jest.fn(() => true)
  }
}

beforeEach(() => {
  handlers.clear()
  onHandlers.clear()
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('input ipc', () => {
  test('projection-sendframe sends SendCommand("frame")', async () => {
    const host = freshHost()
    registerInputIpc(host)
    await handlers.get('projection-sendframe')!(null)
    expect(host.send).toHaveBeenCalledWith(expect.any(SendCommand))
  })

  test('projection-touch forwards a SendTouch', () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-touch')!(null, { x: 0.5, y: 0.5, action: 0 })
    expect(host.send).toHaveBeenCalledWith(expect.any(SendTouch))
  })

  test('projection-touch swallows thrown send', () => {
    const host = freshHost()
    host.send.mockImplementation(() => {
      throw new Error('not started')
    })
    registerInputIpc(host)
    expect(() => onHandlers.get('projection-touch')!(null, { x: 0, y: 0, action: 0 })).not.toThrow()
  })

  test('projection-multi-touch with empty list is a no-op', () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-multi-touch')!(null, [])
    expect(host.send).not.toHaveBeenCalled()
  })

  test('projection-multi-touch sanitizes coordinates to [0,1]', () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-multi-touch')!(null, [
      { id: 0, x: 1.5, y: -0.5, action: 0 },
      { id: 1, x: 0.5, y: NaN, action: 1 }
    ])
    expect(host.send).toHaveBeenCalledWith(expect.any(SendMultiTouch))
  })

  test('projection-multi-touch with non-array is a no-op', () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-multi-touch')!(null, null as never)
    expect(host.send).not.toHaveBeenCalled()
  })

  test('projection-raw-message requires isStarted=true', () => {
    const host = freshHost()
    host.isStarted.mockReturnValue(false)
    registerInputIpc(host)
    onHandlers.get('projection-raw-message')!(null, { type: 1, data: [1, 2, 3] })
    expect(host.send).not.toHaveBeenCalled()
  })

  test('projection-raw-message when started sends a SendRawMessage', () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-raw-message')!(null, { type: 1, data: [1, 2, 3] })
    expect(host.send).toHaveBeenCalledWith(expect.any(SendRawMessage))
  })

  test('projection-command forwards a SendCommand', () => {
    const host = freshHost()
    registerInputIpc(host)
    onHandlers.get('projection-command')!(null, 'play')
    expect(host.send).toHaveBeenCalledWith(expect.any(SendCommand))
  })
})
