import { BrowserWindow } from 'electron'
import { registerIpcHandle } from '@main/ipc/register'
import { Microphone } from '@main/services/audio'

import * as usbModule from 'usb'

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: jest.fn()
}))

jest.mock('@main/services/audio', () => ({
  Microphone: {
    getSysdefaultPrettyName: jest.fn(() => 'System Mic')
  }
}))

jest.mock('usb', () => ({
  usb: {
    on: jest.fn(),
    removeAllListeners: jest.fn(),
    unrefHotplugEvents: jest.fn()
  },
  getDeviceList: jest.fn(() => [])
}))

jest.mock('../helpers', () => ({
  findDongle: jest.fn(() => null)
}))

import { USBService } from '@main/services/usb/USBService'

describe('USBService', () => {
  const getDeviceList = usbModule.getDeviceList as jest.Mock
  const usb = (usbModule as any).usb

  const projection = {
    markDongleConnected: jest.fn(),
    autoStartIfNeeded: jest.fn(async () => undefined),
    stop: jest.fn(async () => undefined)
  } as any

  const mkDevice = (idVendor = 0x1314, idProduct = 0x1520, bcdDevice = 0x0102) =>
    ({
      deviceDescriptor: { idVendor, idProduct, bcdDevice },
      open: jest.fn(),
      close: jest.fn(),
      reset: jest.fn((cb: (err?: unknown) => void) => cb())
    }) as any

  const windows = [
    { webContents: { send: jest.fn() } },
    { webContents: { send: jest.fn() } }
  ] as any[]

  beforeEach(() => {
    jest.clearAllMocks()
    ;(BrowserWindow.getAllWindows as jest.Mock).mockReturnValue(windows)
    getDeviceList.mockReturnValue([])
  })

  function getHandler<T = (...args: unknown[]) => unknown>(channel: string): T {
    const row = (registerIpcHandle as jest.Mock).mock.calls.find(([ch]) => ch === channel)
    if (!row) throw new Error(`Missing handler: ${channel}`)
    return row[1] as T
  }

  test('registers expected ipc handlers', () => {
    new USBService(projection)

    const channels = (registerIpcHandle as jest.Mock).mock.calls.map(([ch]) => ch)
    expect(channels).toEqual(
      expect.arrayContaining([
        'usb-detect-dongle',
        'projection:usbDevice',
        'usb-force-reset',
        'usb-last-event',
        'get-sysdefault-mic-label'
      ])
    )
  })

  test('usb-detect-dongle handler checks known VID/PID devices', async () => {
    new USBService(projection)
    getDeviceList.mockReturnValue([mkDevice(0x1111, 0x2222), mkDevice(0x1314, 0x1521)])

    const h = getHandler<() => Promise<boolean>>('usb-detect-dongle')
    await expect(h()).resolves.toBe(true)
  })

  test('projection:usbDevice returns formatted usb fw version', async () => {
    new USBService(projection)
    getDeviceList.mockReturnValue([mkDevice(0x1314, 0x1520, 0x0110)])

    const h = getHandler<() => Promise<any>>('projection:usbDevice')
    await expect(h()).resolves.toEqual({
      device: true,
      vendorId: 0x1314,
      productId: 0x1520,
      usbFwVersion: '1.16'
    })
  })

  test('get-sysdefault-mic-label proxies static microphone label', () => {
    new USBService(projection)

    const h = getHandler<() => string>('get-sysdefault-mic-label')
    expect(h()).toBe('System Mic')
    expect(Microphone.getSysdefaultPrettyName).toHaveBeenCalledTimes(1)
  })

  test('usb-force-reset uses gracefulReset on darwin', async () => {
    const originalPlatform = process.platform
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    const s = new USBService(projection) as any
    s.gracefulReset = jest.fn(async () => true)

    const h = getHandler<() => Promise<boolean>>('usb-force-reset')
    await expect(h()).resolves.toBe(true)
    expect(s.gracefulReset).toHaveBeenCalledTimes(1)

    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  test('attach event for dongle updates projection and notifies renderer', async () => {
    new USBService(projection)

    const attachCb = (usb.on as jest.Mock).mock.calls.find(
      ([evt]: [string]) => evt === 'attach'
    )?.[1]
    expect(attachCb).toBeDefined()

    const device = mkDevice(0x1314, 0x1520)
    await attachCb(device)

    expect(projection.markDongleConnected).toHaveBeenCalledWith(true)
    expect(projection.autoStartIfNeeded).toHaveBeenCalledTimes(1)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'projection-event',
      expect.objectContaining({ type: 'plugged' })
    )
  })

  test('gracefulReset stops projection and emits reset lifecycle events', async () => {
    const s = new USBService(projection)

    await expect(s.gracefulReset()).resolves.toBe(true)

    expect(projection.stop).toHaveBeenCalledTimes(1)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-start', true)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', true)
  })
})
