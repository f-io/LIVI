import { devicesSchema } from '../devicesSchema'

const schema = devicesSchema as any

describe('devicesSchema', () => {
  test('exposes devices route', () => {
    expect(schema).toEqual(
      expect.objectContaining({
        type: 'route',
        route: 'devices',
        label: 'Devices',
        labelKey: 'settings.devices',
        path: ''
      })
    )
  })

  test('contains btDeviceList leaf', () => {
    expect(schema.children).toEqual([
      expect.objectContaining({
        type: 'btDeviceList',
        path: 'bluetoothPairedDevices'
      })
    ])
  })
})
