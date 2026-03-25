jest.mock('../Media', () => ({
  __esModule: true,
  Media: 'MediaMock'
}))

describe('media index', () => {
  test('re-exports Media module', () => {
    const mod = require('../index')

    expect(mod.Media).toBe('MediaMock')
  })
})
