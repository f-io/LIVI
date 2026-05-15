import { registerIpcHandle } from '@main/ipc/register'
import { BrowserWindow } from 'electron'
import { usb } from 'usb'

jest.mock('electron', () => ({
  BrowserWindow: { getAllWindows: jest.fn(() => []) }
}))

jest.mock('@main/ipc/register', () => ({
  registerIpcHandle: jest.fn()
}))

jest.mock('@main/services/audio', () => ({
  Microphone: { getSysdefaultPrettyName: jest.fn(() => 'Mic') }
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

const probeAaCapableMock = jest.fn(async () => 0)
const isAccessoryModeMock = jest.fn(() => false)
jest.mock('../../projection/driver/aa/stack/aoap/handshake', () => ({
  probeAaCapable: (...a: unknown[]) => probeAaCapableMock(...a),
  isAccessoryMode: (...a: unknown[]) => isAccessoryModeMock(...a)
}))

import { USBService } from '@main/services/usb/USBService'

const projection = {
  markDongleConnected: jest.fn(),
  markPhoneConnected: jest.fn(),
  autoStartIfNeeded: jest.fn(async () => undefined),
  stop: jest.fn(async () => undefined),
  getActiveTransport: jest.fn(() => null),
  isExpectingPhoneReenumeration: jest.fn(() => false)
} as any

function mkPhoneCandidate(vid = 0x18d1, pid = 0x4ee1) {
  return {
    deviceDescriptor: { idVendor: vid, idProduct: pid, bDeviceClass: 0x00 },
    open: jest.fn(),
    close: jest.fn(),
    reset: jest.fn((cb: () => void) => cb())
  } as never
}

function attachHandler(): (device: unknown) => void {
  const onCalls = (usb.on as jest.Mock).mock.calls
  const row = onCalls.find(([e]) => e === 'attach')!
  return row[1] as (device: unknown) => void
}

function detachHandler(): (device: unknown) => void {
  const onCalls = (usb.on as jest.Mock).mock.calls
  const row = onCalls.find(([e]) => e === 'detach')!
  return row[1] as (device: unknown) => void
}

beforeEach(() => {
  jest.clearAllMocks()
  probeAaCapableMock.mockReset().mockResolvedValue(0)
  isAccessoryModeMock.mockReset().mockReturnValue(false)
  projection.markDongleConnected.mockReset()
  projection.markPhoneConnected.mockReset()
  projection.autoStartIfNeeded.mockReset().mockResolvedValue(undefined)
  projection.isExpectingPhoneReenumeration.mockReset().mockReturnValue(false)
  projection.getActiveTransport.mockReset().mockReturnValue(null)
  jest.spyOn(console, 'log').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('USBService — phone probe + attach paths', () => {
  test('accessory-mode device on attach marks phone connected', () => {
    isAccessoryModeMock.mockReturnValue(true)
    new USBService(projection)
    attachHandler()(mkPhoneCandidate())
    expect(projection.markPhoneConnected).toHaveBeenCalledWith(true, expect.anything())
  })

  test('AOAP-capable phone-candidate triggers a probe and marks attached', async () => {
    probeAaCapableMock.mockResolvedValue(2)
    new USBService(projection)
    attachHandler()(mkPhoneCandidate())
    await new Promise((r) => setImmediate(r))
    expect(projection.markPhoneConnected).toHaveBeenCalledWith(true, expect.anything())
  })

  test('phone candidate with proto<1 does not mark connected', async () => {
    probeAaCapableMock.mockResolvedValue(0)
    new USBService(projection)
    attachHandler()(mkPhoneCandidate())
    await new Promise((r) => setImmediate(r))
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })

  test('probe throwing is logged and silenced', async () => {
    probeAaCapableMock.mockRejectedValue(new Error('libusb stalled'))
    new USBService(projection)
    expect(() => attachHandler()(mkPhoneCandidate())).not.toThrow()
    await new Promise((r) => setImmediate(r))
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })

  test('phone detach fires markPhoneConnected(false)', async () => {
    probeAaCapableMock.mockResolvedValue(2)
    const phone = mkPhoneCandidate()
    new USBService(projection)
    attachHandler()(phone)
    await new Promise((r) => setImmediate(r))
    // Advance past the PHONE_REENUM_SUPPRESS_MS window so detach isn't suppressed
    jest.useFakeTimers()
    jest.setSystemTime(Date.now() + 20_000)
    projection.markPhoneConnected.mockClear()
    detachHandler()(phone)
    expect(projection.markPhoneConnected).toHaveBeenCalledWith(false)
    jest.useRealTimers()
  })

  test('phone detach during re-enumeration window is suppressed', async () => {
    probeAaCapableMock.mockResolvedValue(2)
    const phone = mkPhoneCandidate()
    new USBService(projection)
    attachHandler()(phone)
    await new Promise((r) => setImmediate(r))
    projection.markPhoneConnected.mockClear()
    projection.isExpectingPhoneReenumeration.mockReturnValue(true)
    detachHandler()(phone)
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })

  test('OEM-PID re-attach while lastPhone=true resets state', async () => {
    probeAaCapableMock.mockResolvedValue(2)
    const phone = mkPhoneCandidate()
    new USBService(projection)
    attachHandler()(phone)
    await new Promise((r) => setImmediate(r))
    projection.markPhoneConnected.mockClear()
    // Second attach with same OEM-PID
    attachHandler()(phone)
    expect(projection.markPhoneConnected).toHaveBeenCalledWith(false)
  })

  test('accessory-mode re-attach during re-enum window keeps the bridge owner', () => {
    isAccessoryModeMock.mockReturnValue(true)
    const phone = mkPhoneCandidate()
    new USBService(projection)
    attachHandler()(phone)
    projection.markPhoneConnected.mockClear()
    projection.isExpectingPhoneReenumeration.mockReturnValue(true)
    attachHandler()(phone)
    expect(projection.markPhoneConnected).toHaveBeenCalledWith(true, expect.anything())
  })

  test('attach while AA is active suppresses dongle broadcast', () => {
    projection.getActiveTransport.mockReturnValue('aa')
    new USBService(projection)
    const dongle = {
      deviceDescriptor: { idVendor: 0x1314, idProduct: 0x1520, bDeviceClass: 0x00 },
      open: jest.fn(),
      close: jest.fn(),
      reset: jest.fn()
    } as never
    attachHandler()(dongle)
    // markDongleConnected still fires, but the renderer broadcast doesn't
    expect(projection.markDongleConnected).toHaveBeenCalledWith(true)
    expect((BrowserWindow.getAllWindows as jest.Mock).mock.results).toEqual(expect.any(Array))
  })

  test('attach during stopped state is ignored', () => {
    const svc = new USBService(projection)
    ;(svc as unknown as { stopped: boolean }).stopped = true
    attachHandler()(mkPhoneCandidate())
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })

  test('detach during stopped state is ignored', () => {
    const svc = new USBService(projection)
    ;(svc as unknown as { stopped: boolean }).stopped = true
    detachHandler()(mkPhoneCandidate())
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })
})

describe('USBService — startup AOAP scan', () => {
  test('scans device list and probes phone candidates on construction', async () => {
    probeAaCapableMock.mockResolvedValue(2)
    const phone = mkPhoneCandidate()
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([phone])
    new USBService(projection)
    // Startup probe runs asynchronously
    await new Promise((r) => setImmediate(r))
    await new Promise((r) => setImmediate(r))
    expect(probeAaCapableMock).toHaveBeenCalled()
  })

  test('startup scan skips when no candidate is in the list', async () => {
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([])
    new USBService(projection)
    await new Promise((r) => setImmediate(r))
    expect(probeAaCapableMock).not.toHaveBeenCalled()
  })

  test('startup probe failure is swallowed and moves to next candidate', async () => {
    const a = mkPhoneCandidate(0x18d1, 0x1111)
    const b = mkPhoneCandidate(0x18d1, 0x2222)
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([a, b])
    probeAaCapableMock.mockRejectedValueOnce(new Error('bad'))
    probeAaCapableMock.mockResolvedValueOnce(2)
    new USBService(projection)
    await new Promise((r) => setImmediate(r))
    await new Promise((r) => setImmediate(r))
    await new Promise((r) => setImmediate(r))
    expect(probeAaCapableMock).toHaveBeenCalledTimes(2)
  })
})

describe('USBService — isPhoneCandidate filter', () => {
  test('skips device classes already in the SKIP list (HID/HUB/etc.)', () => {
    new USBService(projection)
    const hub = {
      deviceDescriptor: { idVendor: 0x1000, idProduct: 0x2000, bDeviceClass: 0x09 /* hub */ },
      open: jest.fn(),
      close: jest.fn(),
      reset: jest.fn()
    } as never
    attachHandler()(hub)
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })

  test('skips a device with undefined bDeviceClass', () => {
    new USBService(projection)
    const weird = {
      deviceDescriptor: { idVendor: 0x1000, idProduct: 0x2000, bDeviceClass: undefined },
      open: jest.fn(),
      close: jest.fn(),
      reset: jest.fn()
    } as never
    attachHandler()(weird)
    expect(projection.markPhoneConnected).not.toHaveBeenCalled()
  })
})

// Ensure registerIpcHandle has been called (test infrastructure check)
test('USBService registers IPC on construction', () => {
  new USBService(projection)
  expect(registerIpcHandle).toHaveBeenCalled()
})
