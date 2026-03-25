describe('home index', () => {
  test('re-exports Home', () => {
    const mod = require('../index')

    expect(mod).toHaveProperty('Home')
  })
})
