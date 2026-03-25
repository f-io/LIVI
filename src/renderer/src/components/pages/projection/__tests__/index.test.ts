jest.mock('../Projection', () => ({
  __esModule: true,
  Projection: 'ProjectionMock'
}))

describe('projection index', () => {
  test('re-exports Projection module', () => {
    const mod = require('../index')

    expect(mod.Projection).toBe('ProjectionMock')
  })
})
