import { WebUSBDevice } from 'usb'

export function asDomUSBDevice(dev: WebUSBDevice): USBDevice {
  const d = dev as unknown as USBDevice & {
    manufacturerName?: string | null
    productName?: string | null
    serialNumber?: string | null
  }
  if (d.manufacturerName === undefined) d.manufacturerName = null
  if (d.productName === undefined) d.productName = null
  if (d.serialNumber === undefined) d.serialNumber = null
  return d as unknown as USBDevice
}
