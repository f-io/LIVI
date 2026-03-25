jest.mock('../keysControl', () => ({
  __esModule: true,
  useActivateControl: 'useActivateControlMock',
  useFocus: 'useFocusMock',
  useKeyDown: 'useKeyDownMock'
}))

jest.mock('../useNavbarHidden', () => ({
  __esModule: true,
  useNavbarHidden: 'useNavbarHiddenMock'
}))

describe('hooks index', () => {
  test('re-exports hooks', () => {
    const mod = require('../index')

    expect(mod.useActivateControl).toBe('useActivateControlMock')
    expect(mod.useFocus).toBe('useFocusMock')
    expect(mod.useKeyDown).toBe('useKeyDownMock')
    expect(mod.useNavbarHidden).toBe('useNavbarHiddenMock')
  })
})
