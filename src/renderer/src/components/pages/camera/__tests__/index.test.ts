jest.mock('../Camera', () => ({
  __esModule: true,
  Camera: 'CameraMock'
}))

describe('camera index', () => {
  test('re-exports Camera module', () => {
    const mod = require('../index')

    expect(mod.Camera).toBe('CameraMock')
  })
})
