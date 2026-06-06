import { usb } from 'usb'
import { isCarlinkitDongle } from './constants'

export async function findDongle() {
  const devices = await usb.getDevices()
  return devices.find((d) => isCarlinkitDongle(d.vendorId, d.productId)) ?? null
}
