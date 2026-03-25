describe('debug index', () => {
  test('re-exports Debug', () => {
    const mod = require('../index')

    expect(mod).toHaveProperty('Debug')
  })
})
