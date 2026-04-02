import { findDongle } from '@main/services/usb/helpers'
import { usb } from 'usb'

jest.mock('usb', () => ({
  usb: {
    getDeviceList: jest.fn(() => [])
  }
}))

describe('findDongle', () => {
  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('returns matching dongle when supported VID/PID is present', () => {
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([
      { deviceDescriptor: { idVendor: 0x1111, idProduct: 0x2222 } },
      { deviceDescriptor: { idVendor: 0x1314, idProduct: 0x1521 } }
    ])

    const found = findDongle()
    expect(found).toEqual({ deviceDescriptor: { idVendor: 0x1314, idProduct: 0x1521 } })
  })

  test('returns null when no matching dongle found', () => {
    ;(usb.getDeviceList as jest.Mock).mockReturnValue([
      { deviceDescriptor: { idVendor: 0x1111, idProduct: 0x2222 } }
    ])

    expect(findDongle()).toBeNull()
  })
})
