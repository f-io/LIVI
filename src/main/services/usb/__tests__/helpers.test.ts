import { findDongle } from '@main/services/usb/helpers'
import { usb } from 'usb'

jest.mock('usb', () => ({
  usb: {
    getDevices: jest.fn(async () => [])
  }
}))

describe('findDongle', () => {
  const getDevices = usb.getDevices as jest.Mock

  beforeEach(() => {
    jest.clearAllMocks()
  })

  test('returns matching dongle when supported VID/PID is present', async () => {
    const dongle = { vendorId: 0x1314, productId: 0x1521 }
    getDevices.mockResolvedValue([{ vendorId: 0x1111, productId: 0x2222 }, dongle])

    const found = await findDongle()
    expect(found).toBe(dongle)
  })

  test('returns null when no matching dongle found', async () => {
    getDevices.mockResolvedValue([{ vendorId: 0x1111, productId: 0x2222 }])

    await expect(findDongle()).resolves.toBeNull()
  })
})
