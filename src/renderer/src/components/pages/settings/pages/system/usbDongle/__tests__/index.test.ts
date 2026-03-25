describe('usbDongle index', () => {
  test('re-exports USBDongle', () => {
    const mod = require('../index')

    expect(mod).toHaveProperty('USBDongle')
  })
})
