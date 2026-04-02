import { registerIpcHandle } from '@main/ipc/register'
import { Microphone } from '@main/services/audio'
import { USBService } from '@main/services/usb/USBService'
import { BrowserWindow } from 'electron'
import { usb } from 'usb'
import { findDongle } from '../helpers'

jest.mock('electron', () => ({
  BrowserWindow: {
    getAllWindows: jest.fn(() => [])
  }
}))

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
    unrefHotplugEvents: jest.fn(),
    getDeviceList: jest.fn(() => [])
  }
}))

jest.mock('../helpers', () => ({
  findDongle: jest.fn(() => null)
}))

describe('USBService', () => {
  const getDeviceList = usb.getDeviceList as jest.Mock
  const mockedFindDongle = findDongle as jest.Mock

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

  const originalPlatform = process.platform

  beforeEach(() => {
    jest.clearAllMocks()
    jest.useFakeTimers()
    ;(BrowserWindow.getAllWindows as jest.Mock).mockReturnValue(windows)
    getDeviceList.mockReturnValue([])
    mockedFindDongle.mockReturnValue(null)
  })

  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
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

  test('constructor detects already connected dongle on startup', () => {
    getDeviceList.mockReturnValue([mkDevice(0x1314, 0x1520)])

    new USBService(projection)

    expect(projection.markDongleConnected).toHaveBeenCalledWith(true)
    expect(projection.autoStartIfNeeded).toHaveBeenCalledTimes(1)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'usb-event',
      expect.objectContaining({ type: 'plugged' })
    )
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'projection-event',
      expect.objectContaining({ type: 'plugged' })
    )
  })

  test('constructor unrefs hotplug events on non-darwin platforms', () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    new USBService(projection)

    expect(usb.unrefHotplugEvents).toHaveBeenCalledTimes(1)
  })

  test('constructor does not unref hotplug events on darwin', () => {
    Object.defineProperty(process, 'platform', { value: 'darwin' })

    new USBService(projection)

    expect(usb.unrefHotplugEvents).not.toHaveBeenCalled()
  })

  test('usb-detect-dongle handler checks known VID/PID devices', async () => {
    new USBService(projection)
    getDeviceList.mockReturnValue([mkDevice(0x1111, 0x2222), mkDevice(0x1314, 0x1521)])

    const h = getHandler<() => Promise<boolean>>('usb-detect-dongle')
    await expect(h()).resolves.toBe(true)
  })

  test('usb-detect-dongle returns false during shutdown or reset', async () => {
    const s = new USBService(projection) as any
    const h = getHandler<() => Promise<boolean>>('usb-detect-dongle')

    s.shutdownInProgress = true
    await expect(h()).resolves.toBe(false)

    s.shutdownInProgress = false
    s.resetInProgress = true
    await expect(h()).resolves.toBe(false)
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

  test('projection:usbDevice returns empty info during shutdown/reset or when no dongle exists', async () => {
    const s = new USBService(projection) as any
    const h = getHandler<() => Promise<any>>('projection:usbDevice')

    s.shutdownInProgress = true
    await expect(h()).resolves.toEqual({
      device: false,
      vendorId: null,
      productId: null,
      usbFwVersion: 'Unknown'
    })

    s.shutdownInProgress = false
    s.resetInProgress = true
    await expect(h()).resolves.toEqual({
      device: false,
      vendorId: null,
      productId: null,
      usbFwVersion: 'Unknown'
    })

    s.resetInProgress = false
    getDeviceList.mockReturnValue([])
    await expect(h()).resolves.toEqual({
      device: false,
      vendorId: null,
      productId: null,
      usbFwVersion: 'Unknown'
    })
  })

  test('usb-last-event returns plugged payload when last dongle is still present', async () => {
    const s = new USBService(projection) as any
    s.lastDongleState = true
    getDeviceList.mockReturnValue([mkDevice(0x1314, 0x1521)])

    const h = getHandler<() => Promise<any>>('usb-last-event')

    await expect(h()).resolves.toEqual({
      type: 'plugged',
      device: {
        vendorId: 0x1314,
        productId: 0x1521,
        deviceName: ''
      }
    })
  })

  test('usb-last-event returns unplugged during shutdown/reset or when dongle is absent', async () => {
    const s = new USBService(projection) as any
    const h = getHandler<() => Promise<any>>('usb-last-event')

    s.shutdownInProgress = true
    await expect(h()).resolves.toEqual({ type: 'unplugged', device: null })

    s.shutdownInProgress = false
    s.resetInProgress = true
    await expect(h()).resolves.toEqual({ type: 'unplugged', device: null })

    s.resetInProgress = false
    s.lastDongleState = false
    await expect(h()).resolves.toEqual({ type: 'unplugged', device: null })
  })

  test('get-sysdefault-mic-label proxies static microphone label', () => {
    new USBService(projection)

    const h = getHandler<() => string>('get-sysdefault-mic-label')
    expect(h()).toBe('System Mic')
    expect(Microphone.getSysdefaultPrettyName).toHaveBeenCalledTimes(1)
  })

  test('usb-force-reset uses forceReset on darwin', async () => {
    Object.defineProperty(process, 'platform', {
      value: 'darwin',
      configurable: true
    })

    const s = new USBService(projection) as any
    s.forceReset = jest.fn(async () => true)

    const h = getHandler<() => Promise<boolean>>('usb-force-reset')

    await expect(h()).resolves.toBe(true)
    expect(s.forceReset).toHaveBeenCalledTimes(1)
  })

  test('usb-force-reset uses forceReset on non-darwin', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux' })

    const s = new USBService(projection) as any
    s.forceReset = jest.fn(async () => true)

    const h = getHandler<() => Promise<boolean>>('usb-force-reset')
    await expect(h()).resolves.toBe(true)
    expect(s.forceReset).toHaveBeenCalledTimes(1)
  })

  test('usb-force-reset returns false when shutdown or reset already in progress', async () => {
    const s = new USBService(projection) as any
    const h = getHandler<() => Promise<boolean>>('usb-force-reset')

    s.shutdownInProgress = true
    await expect(h()).resolves.toBe(false)

    s.shutdownInProgress = false
    s.resetInProgress = true
    await expect(h()).resolves.toBe(false)
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

  test('attach event ignores non-dongle devices', async () => {
    new USBService(projection)

    const attachCb = (usb.on as jest.Mock).mock.calls.find(
      ([evt]: [string]) => evt === 'attach'
    )?.[1]

    await attachCb(mkDevice(0x1111, 0x2222))

    expect(projection.markDongleConnected).not.toHaveBeenCalled()
    expect(projection.autoStartIfNeeded).not.toHaveBeenCalled()
  })

  test('attach event is ignored when stopped, resetting or shutting down', async () => {
    const s = new USBService(projection) as any

    const attachCb = (usb.on as jest.Mock).mock.calls.find(
      ([evt]: [string]) => evt === 'attach'
    )?.[1]

    s.stopped = true
    await attachCb(mkDevice())
    expect(projection.markDongleConnected).not.toHaveBeenCalled()

    s.stopped = false
    s.resetInProgress = true
    await attachCb(mkDevice())
    expect(projection.markDongleConnected).not.toHaveBeenCalled()

    s.resetInProgress = false
    s.shutdownInProgress = true
    await attachCb(mkDevice())
    expect(projection.markDongleConnected).not.toHaveBeenCalled()
  })

  test('detach event for dongle updates projection and notifies renderer', async () => {
    const s = new USBService(projection) as any
    s.lastDongleState = true

    const detachCb = (usb.on as jest.Mock).mock.calls.find(
      ([evt]: [string]) => evt === 'detach'
    )?.[1]
    expect(detachCb).toBeDefined()

    await detachCb(mkDevice(0x1314, 0x1520))

    expect(projection.markDongleConnected).toHaveBeenCalledWith(false)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'projection-event',
      expect.objectContaining({ type: 'unplugged' })
    )
  })

  test('stop removes attach/detach listeners and is idempotent', async () => {
    const s = new USBService(projection)

    await s.stop()
    await s.stop()

    expect(usb.removeAllListeners).toHaveBeenCalledWith('attach')
    expect(usb.removeAllListeners).toHaveBeenCalledWith('detach')
  })

  test('forceReset returns false when shutdown/reset already active', async () => {
    const s = new USBService(projection) as any

    s.shutdownInProgress = true
    await expect(s.forceReset()).resolves.toBe(false)

    s.shutdownInProgress = false
    s.resetInProgress = true
    await expect(s.forceReset()).resolves.toBe(false)
  })

  test('forceReset handles missing dongle and emits detach without device', async () => {
    const s = new USBService(projection) as any
    mockedFindDongle.mockReturnValue(null)

    const promise = s.forceReset()

    await Promise.resolve()
    await Promise.resolve()

    await jest.advanceTimersByTimeAsync(200)

    await expect(promise).resolves.toBe(false)

    expect(projection.stop).toHaveBeenCalledTimes(1)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-start', true)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'usb-event',
      expect.objectContaining({
        type: 'detach',
        device: { vendorId: null, productId: null, deviceName: '' }
      })
    )
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'projection-event',
      expect.objectContaining({
        type: 'unplugged',
        device: { vendorId: null, productId: null, deviceName: '' }
      })
    )
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', false)
    expect(s.resetInProgress).toBe(false)
  })

  test('forceReset resets dongle when found and notifies detach for concrete device', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice(0x1314, 0x1520)
    mockedFindDongle.mockReturnValue(dongle)
    s.resetDongle = jest.fn(async () => true)

    const promise = s.forceReset()
    await jest.advanceTimersByTimeAsync(200)

    await expect(promise).resolves.toBe(true)

    expect(s.resetDongle).toHaveBeenCalledWith(dongle)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'usb-event',
      expect.objectContaining({
        type: 'detach',
        device: { vendorId: 0x1314, productId: 0x1520, deviceName: '' }
      })
    )
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'projection-event',
      expect.objectContaining({
        type: 'unplugged',
        device: { vendorId: 0x1314, productId: 0x1520, deviceName: '' }
      })
    )
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', true)
  })

  test('forceReset returns false when projection.stop throws', async () => {
    const s = new USBService({
      ...projection,
      stop: jest.fn(async () => {
        throw new Error('stop failed')
      })
    } as any)

    const promise = s.forceReset()
    await jest.advanceTimersByTimeAsync(200)

    await expect(promise).resolves.toBe(false)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', false)
  })

  test('gracefulReset stops projection and emits reset lifecycle events', async () => {
    const s = new USBService(projection)

    const promise = s.gracefulReset()
    await jest.advanceTimersByTimeAsync(400)

    await expect(promise).resolves.toBe(true)

    expect(projection.stop).toHaveBeenCalledTimes(1)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-start', true)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', true)
  })

  test('gracefulReset returns false when projection stop throws', async () => {
    const s = new USBService({
      ...projection,
      stop: jest.fn(async () => {
        throw new Error('boom')
      })
    } as any)

    const promise = s.gracefulReset()
    await jest.advanceTimersByTimeAsync(400)

    await expect(promise).resolves.toBe(false)
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', false)
  })

  test('resetDongle returns false when device open fails', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice()
    dongle.open.mockImplementation(() => {
      throw new Error('cannot open')
    })

    await expect(s.resetDongle(dongle)).resolves.toBe(false)
  })

  test('resetDongle treats LIBUSB disconnect errors as success', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice()
    dongle.reset = jest.fn((cb: (err?: unknown) => void) => cb(new Error('LIBUSB_ERROR_NO_DEVICE')))

    await expect(s.resetDongle(dongle)).resolves.toBe(true)
    expect(dongle.close).toHaveBeenCalledTimes(1)
  })

  test('resetDongle returns false on real reset error and still closes device', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice()
    dongle.reset = jest.fn((cb: (err?: unknown) => void) => cb(new Error('real reset error')))

    await expect(s.resetDongle(dongle)).resolves.toBe(false)
    expect(dongle.close).toHaveBeenCalledTimes(1)
  })
})
