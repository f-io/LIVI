import usb from 'usb'

export function findDongle() {
  return (
    usb
      .getDeviceList()
      .find(
        (d) =>
          d.deviceDescriptor.idVendor === 0x1314 &&
          [0x1520, 0x1521].includes(d.deviceDescriptor.idProduct)
      ) ?? null
  )
}
