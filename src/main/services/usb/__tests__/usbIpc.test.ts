type IpcHandler = (evt: unknown, ...args: unknown[]) => unknown
const handlers = new Map<string, IpcHandler>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: (channel: string, handler: IpcHandler) => {
    handlers.set(channel, handler)
  },
  registerIpcOn: vi.fn()
}))

import { registerUsbIpc } from '../usbIpc'

beforeEach(() => {
  handlers.clear()
})

describe('usb ipc', () => {
  test('registers get-sysdefault-mic-label returning the system default label', () => {
    registerUsbIpc()
    expect(handlers.get('get-sysdefault-mic-label')!(null)).toBe('system default')
  })
})
