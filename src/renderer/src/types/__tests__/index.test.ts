jest.mock('../dongle', () => ({
  __esModule: true,
  DongleType: 'DongleTypeMock'
}))

jest.mock('../fw', () => ({
  __esModule: true,
  FirmwareType: 'FirmwareTypeMock'
}))

jest.mock('../ui', () => ({
  __esModule: true,
  UIType: 'UITypeMock'
}))

describe('types index', () => {
  test('re-exports types modules', () => {
    const mod = require('../index')

    expect(mod.DongleType).toBe('DongleTypeMock')
    expect(mod.FirmwareType).toBe('FirmwareTypeMock')
    expect(mod.UIType).toBe('UITypeMock')
  })
})
