import { afterEach, describe, expect, test, vi } from 'vitest'

const handlers = new Map<string, (...args: unknown[]) => unknown>()

vi.mock('@main/ipc/register', () => ({
  registerIpcHandle: vi.fn((channel: string, handler: (...args: unknown[]) => unknown) => {
    handlers.set(channel, handler)
  })
}))

import type { DongleDriver } from '@main/services/projection/driver/dongle/dongleDriver'
import { registerUsbIpc } from '../usbIpc'

type Fake = {
  isUp: boolean
  usbDevice: ReturnType<typeof vi.fn>
  resetDongle: ReturnType<typeof vi.fn>
}

function dongle(up: boolean): Fake {
  return {
    isUp: up,
    usbDevice: vi.fn(() =>
      up
        ? { vendorId: 0x1314, productId: 0x1520, usbFwVersion: '1.00', deviceName: 'Carlinkit' }
        : null
    ),
    resetDongle: vi.fn(() => up)
  }
}

function register(d: Fake): (channel: string) => Promise<unknown> {
  registerUsbIpc(() => d as unknown as DongleDriver)
  return (channel) => Promise.resolve(handlers.get(channel)?.({}))
}

afterEach(() => handlers.clear())

describe('registerUsbIpc', () => {
  test('answers every USB query from the dongle session', async () => {
    const call = register(dongle(true))
    expect([...handlers.keys()].sort()).toEqual([
      'get-sysdefault-mic-label',
      'projection:usbDevice',
      'usb-detect-dongle',
      'usb-force-reset',
      'usb-last-event'
    ])
    expect(await call('usb-detect-dongle')).toBe(true)
    expect(await call('projection:usbDevice')).toEqual({
      device: true,
      vendorId: 0x1314,
      productId: 0x1520,
      usbFwVersion: '1.00'
    })
    expect(await call('usb-last-event')).toEqual({
      type: 'plugged',
      device: { vendorId: 0x1314, productId: 0x1520, deviceName: 'Carlinkit' }
    })
    expect(await call('usb-force-reset')).toBe(true)
    expect(await call('get-sysdefault-mic-label')).toBe('system default')
  })

  test('reports an empty bus while no dongle session is up', async () => {
    const call = register(dongle(false))
    expect(await call('usb-detect-dongle')).toBe(false)
    expect(await call('projection:usbDevice')).toEqual({
      device: false,
      vendorId: null,
      productId: null,
      usbFwVersion: 'Unknown'
    })
    expect(await call('usb-last-event')).toEqual({ type: 'unplugged', device: null })
    expect(await call('usb-force-reset')).toBe(false)
  })
})
