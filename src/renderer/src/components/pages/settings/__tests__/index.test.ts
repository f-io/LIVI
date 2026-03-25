jest.mock('../SettingsPage', () => ({
  __esModule: true,
  SettingsPage: 'SettingsPageMock'
}))

describe('settings index', () => {
  test('re-exports SettingsPage module', () => {
    const mod = require('../index')

    expect(mod.SettingsPage).toBe('SettingsPageMock')
  })
})
