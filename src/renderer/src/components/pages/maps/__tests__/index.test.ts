jest.mock('../Maps', () => ({
  __esModule: true,
  Maps: 'MapsMock'
}))

describe('maps index', () => {
  test('re-exports Maps module', () => {
    const mod = require('../index')

    expect(mod.Maps).toBe('MapsMock')
  })
})
