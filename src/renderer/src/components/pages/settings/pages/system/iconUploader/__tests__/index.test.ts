describe('iconUploader index', () => {
  test('re-exports IconUploader', () => {
    const mod = require('../index')

    expect(mod).toHaveProperty('IconUploader')
  })
})
