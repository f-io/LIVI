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
    addEventListener: jest.fn(),
    removeEventListener: jest.fn(),
    getDevices: jest.fn(async () => [])
  }
}))

jest.mock('../../projection/driver/aa/stack/aoap/handshake', () => ({
  probeAaCapable: jest.fn(async () => 0),
  isAccessoryMode: jest.fn(() => false)
}))

jest.mock('../helpers', () => ({
  findDongle: jest.fn(async () => null)
}))

describe('USBService', () => {
  const getDevices = usb.getDevices as jest.Mock
  const addEventListener = usb.addEventListener as jest.Mock
  const mockedFindDongle = findDongle as jest.Mock

  const projection = {
    markDongleConnected: jest.fn(),
    markPhoneConnected: jest.fn(),
    autoStartIfNeeded: jest.fn(async () => undefined),
    stop: jest.fn(async () => undefined),
    getActiveTransport: jest.fn(() => null),
    isExpectingPhoneReenumeration: jest.fn(() => false)
  } as any

  // usb@3 USBDevice: flat fields, async methods.
  const mkDevice = (
    vendorId = 0x1314,
    productId = 0x1520,
    { major = 1, minor = 0, subminor = 2, deviceClass = 0x00 } = {}
  ) =>
    ({
      vendorId,
      productId,
      deviceClass,
      deviceVersionMajor: major,
      deviceVersionMinor: minor,
      deviceVersionSubminor: subminor,
      open: jest.fn(async () => undefined),
      close: jest.fn(async () => undefined),
      reset: jest.fn(async () => undefined)
    }) as any

  // A USBConnectionEvent wraps the device under `.device`.
  const evt = (device: unknown) => ({ device }) as any

  const getConnectCb = () =>
    addEventListener.mock.calls.find(([e]: [string]) => e === 'connect')?.[1]
  const getDisconnectCb = () =>
    addEventListener.mock.calls.find(([e]: [string]) => e === 'disconnect')?.[1]

  const windows = [
    { webContents: { send: jest.fn() } },
    { webContents: { send: jest.fn() } }
  ] as any[]

  const originalPlatform = process.platform

  beforeEach(() => {
    jest.clearAllMocks()
    jest.useFakeTimers()
    ;(BrowserWindow.getAllWindows as jest.Mock).mockReturnValue(windows)
    getDevices.mockResolvedValue([])
    mockedFindDongle.mockResolvedValue(null)
  })

  afterEach(() => {
    jest.runOnlyPendingTimers()
    jest.useRealTimers()
    Object.defineProperty(process, 'platform', { value: originalPlatform })
  })

  const flush = () => Promise.resolve().then(() => Promise.resolve())

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

  test('constructor detects already connected dongle on startup', async () => {
    getDevices.mockResolvedValue([mkDevice(0x1314, 0x1520)])

    new USBService(projection)
    await flush()

    expect(projection.markDongleConnected).toHaveBeenCalledWith(true)
    // Session start is owned by main/index.ts after applyConfigPatch — the
    // constructor must not race past that with its own autoStart call.
    expect(projection.autoStartIfNeeded).not.toHaveBeenCalled()
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'usb-event',
      expect.objectContaining({ type: 'plugged' })
    )
    // USBService no longer drives projection-event lifecycle — that comes from
    // the active driver's AAP messages via ProjectionService.
    expect(windows[0].webContents.send).not.toHaveBeenCalledWith(
      'projection-event',
      expect.any(Object)
    )
  })

  test('constructor registers connect/disconnect hotplug listeners', () => {
    new USBService(projection)

    expect(addEventListener).toHaveBeenCalledWith('connect', expect.any(Function))
    expect(addEventListener).toHaveBeenCalledWith('disconnect', expect.any(Function))
  })

  test('usb-detect-dongle handler checks known VID/PID devices', async () => {
    new USBService(projection)
    getDevices.mockResolvedValue([mkDevice(0x1111, 0x2222), mkDevice(0x1314, 0x1521)])

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
    // bcdDevice 1.16 → major=1, lowByte=(minor<<4)|subminor=0x10=16
    getDevices.mockResolvedValue([mkDevice(0x1314, 0x1520, { major: 1, minor: 1, subminor: 0 })])

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
    getDevices.mockResolvedValue([])
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
    getDevices.mockResolvedValue([mkDevice(0x1314, 0x1521)])

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

  test('usb-force-reset delegates to forceReset', async () => {
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

  test('connect event for dongle updates projection and notifies renderer', async () => {
    new USBService(projection)

    const connectCb = getConnectCb()
    expect(connectCb).toBeDefined()

    connectCb(evt(mkDevice(0x1314, 0x1520)))

    expect(projection.markDongleConnected).toHaveBeenCalledWith(true)
    expect(projection.autoStartIfNeeded).toHaveBeenCalledTimes(1)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'usb-event',
      expect.objectContaining({ type: 'plugged' })
    )
    // Session lifecycle on projection-event is owned by the active driver,
    // not the USB layer.
    expect(windows[0].webContents.send).not.toHaveBeenCalledWith(
      'projection-event',
      expect.any(Object)
    )
  })

  test('connect event ignores non-dongle, non-phone-candidate devices', () => {
    new USBService(projection)

    const connectCb = getConnectCb()
    // deviceClass 0x09 (hub) is a skip class, so no probe/markPhoneConnected
    connectCb(evt(mkDevice(0x1111, 0x2222, { deviceClass: 0x09 })))

    expect(projection.markDongleConnected).not.toHaveBeenCalled()
    expect(projection.autoStartIfNeeded).not.toHaveBeenCalled()
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })

  test('connect event is ignored when stopped, resetting or shutting down', () => {
    const s = new USBService(projection) as any

    const connectCb = getConnectCb()

    s.stopped = true
    connectCb(evt(mkDevice()))
    expect(projection.markDongleConnected).not.toHaveBeenCalled()

    s.stopped = false
    s.resetInProgress = true
    connectCb(evt(mkDevice()))
    expect(projection.markDongleConnected).not.toHaveBeenCalled()

    s.resetInProgress = false
    s.shutdownInProgress = true
    connectCb(evt(mkDevice()))
    expect(projection.markDongleConnected).not.toHaveBeenCalled()
  })

  test('disconnect event for dongle updates projection and notifies renderer', () => {
    const s = new USBService(projection) as any
    s.lastDongleState = true

    const disconnectCb = getDisconnectCb()
    expect(disconnectCb).toBeDefined()

    disconnectCb(evt(mkDevice(0x1314, 0x1520)))

    expect(projection.markDongleConnected).toHaveBeenCalledWith(false)
    expect(windows[0].webContents.send).toHaveBeenCalledWith(
      'usb-event',
      expect.objectContaining({ type: 'unplugged' })
    )
    // No projection-event lifecycle from USB — session loss propagates from
    // the active driver only.
    expect(windows[0].webContents.send).not.toHaveBeenCalledWith(
      'projection-event',
      expect.any(Object)
    )
  })

  test('stop removes connect/disconnect listeners and is idempotent', async () => {
    const s = new USBService(projection)

    await s.stop()
    await s.stop()

    expect(usb.removeEventListener).toHaveBeenCalledWith('connect', expect.any(Function))
    expect(usb.removeEventListener).toHaveBeenCalledWith('disconnect', expect.any(Function))
    // Idempotent: handlers are cleared after the first stop, so removeEventListener
    // is not invoked again.
    expect((usb.removeEventListener as jest.Mock).mock.calls.length).toBe(2)
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
    mockedFindDongle.mockResolvedValue(null)

    const promise = s.forceReset()

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
    expect(windows[0].webContents.send).not.toHaveBeenCalledWith(
      'projection-event',
      expect.any(Object)
    )
    expect(windows[0].webContents.send).toHaveBeenCalledWith('usb-reset-done', false)
    expect(s.resetInProgress).toBe(false)
  })

  test('forceReset resets dongle when found and notifies detach for concrete device', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice(0x1314, 0x1520)
    mockedFindDongle.mockResolvedValue(dongle)
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
    expect(windows[0].webContents.send).not.toHaveBeenCalledWith(
      'projection-event',
      expect.any(Object)
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
    dongle.open.mockRejectedValue(new Error('cannot open'))

    await expect(s.resetDongle(dongle)).resolves.toBe(false)
  })

  test('resetDongle treats disconnect errors as success', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice()
    dongle.reset.mockRejectedValue(new Error('LIBUSB_ERROR_NO_DEVICE'))

    await expect(s.resetDongle(dongle)).resolves.toBe(true)
    expect(dongle.close).toHaveBeenCalledTimes(1)
  })

  test('resetDongle returns false on real reset error and still closes device', async () => {
    const s = new USBService(projection) as any
    const dongle = mkDevice()
    dongle.reset.mockRejectedValue(new Error('real reset error'))

    await expect(s.resetDongle(dongle)).resolves.toBe(false)
    expect(dongle.close).toHaveBeenCalledTimes(1)
  })
})
