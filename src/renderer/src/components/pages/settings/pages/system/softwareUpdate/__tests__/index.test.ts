describe('softwareUpdate index', () => {
  test('re-exports SoftwareUpdate', () => {
    const mod = require('../index')

    expect(mod).toHaveProperty('SoftwareUpdate')
  })
})
