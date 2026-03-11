import { asDomUSBDevice } from '@main/services/projection/services/utils/asDomUSBDevice'

describe('asDomUSBDevice', () => {
  test('fills missing manufacturer/product/serial fields with null', () => {
    const device = {} as any

    const out = asDomUSBDevice(device)

    expect(out).toBe(device)
    expect((out as any).manufacturerName).toBeNull()
    expect((out as any).productName).toBeNull()
    expect((out as any).serialNumber).toBeNull()
  })

  test('keeps already-present values intact', () => {
    const device = {
      manufacturerName: 'ACME',
      productName: 'Dongle',
      serialNumber: '123'
    } as any

    const out = asDomUSBDevice(device)

    expect((out as any).manufacturerName).toBe('ACME')
    expect((out as any).productName).toBe('Dongle')
    expect((out as any).serialNumber).toBe('123')
  })
})
