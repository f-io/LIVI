vi.mock('../keysControl', () => ({
  __esModule: true,
  useActivateControl: 'useActivateControlMock',
  useFocus: 'useFocusMock',
  useKeyDown: 'useKeyDownMock'
}))

vi.mock('../useFollowFront', () => ({
  __esModule: true,
  useFollowFront: 'useFollowFrontMock'
}))

vi.mock('../useNavbarHidden', () => ({
  __esModule: true,
  useNavbarHidden: 'useNavbarHiddenMock'
}))

describe('hooks index', () => {
  test('re-exports hooks', async () => {
    const mod = await import('../index')

    expect(mod.useActivateControl).toBe('useActivateControlMock')
    expect(mod.useFocus).toBe('useFocusMock')
    expect(mod.useKeyDown).toBe('useKeyDownMock')
    expect(mod.useFollowFront).toBe('useFollowFrontMock')
    expect(mod.useNavbarHidden).toBe('useNavbarHiddenMock')
  })
})
