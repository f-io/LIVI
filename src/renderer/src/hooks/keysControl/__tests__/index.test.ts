jest.mock('../useActivateControl', () => ({
  __esModule: true,
  useActivateControl: 'useActivateControlMock'
}))

jest.mock('../useKeyDown', () => ({
  __esModule: true,
  useKeyDown: 'useKeyDownMock'
}))

jest.mock('../useFocus', () => ({
  __esModule: true,
  useFocus: 'useFocusMock'
}))

describe('keysControl index', () => {
  test('re-exports key control hooks', () => {
    const mod = require('../index')

    expect(mod.useActivateControl).toBe('useActivateControlMock')
    expect(mod.useKeyDown).toBe('useKeyDownMock')
    expect(mod.useFocus).toBe('useFocusMock')
  })
})
