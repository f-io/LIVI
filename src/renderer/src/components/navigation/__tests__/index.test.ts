describe('navigation index', () => {
  test('re-exports Nav module', () => {
    const mod = require('../index')
    expect(mod).toHaveProperty('Nav')
  })
})
