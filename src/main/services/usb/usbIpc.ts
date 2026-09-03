import { registerIpcHandle } from '@main/ipc/register'
import type { DongleDriver } from '@main/services/projection/driver/dongle/dongleDriver'

/** The renderer's USB queries, answered from the dongle session the helper carries. */
export function registerUsbIpc(getDongle: () => DongleDriver): void {
  registerIpcHandle('usb-detect-dongle', async () => getDongle().isUp)

  registerIpcHandle('projection:usbDevice', async () => {
    const dev = getDongle().usbDevice()
    if (!dev) return { device: false, vendorId: null, productId: null, usbFwVersion: 'Unknown' }
    return {
      device: true,
      vendorId: dev.vendorId,
      productId: dev.productId,
      usbFwVersion: dev.usbFwVersion
    }
  })

  registerIpcHandle('usb-force-reset', async () => getDongle().resetDongle())

  registerIpcHandle('usb-last-event', async () => {
    const dev = getDongle().usbDevice()
    if (!dev) return { type: 'unplugged', device: null }
    return {
      type: 'plugged',
      device: { vendorId: dev.vendorId, productId: dev.productId, deviceName: dev.deviceName }
    }
  })

  registerIpcHandle('get-sysdefault-mic-label', () => 'system default')
}
