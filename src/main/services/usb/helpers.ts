import { usb } from 'usb'
import { isCarlinkitDongle } from './constants'

export function findDongle() {
  return (
    usb
      .getDeviceList()
      .find((d) => isCarlinkitDongle(d.deviceDescriptor.idVendor, d.deviceDescriptor.idProduct)) ??
    null
  )
}
