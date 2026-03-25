jest.mock('../appRoutes', () => ({
  __esModule: true,
  appRoutes: 'appRoutesMock'
}))

jest.mock('../types', () => ({
  __esModule: true,
  RouteType: 'RouteTypeMock'
}))

describe('routes index', () => {
  test('re-exports route modules', () => {
    const mod = require('../index')

    expect(mod.appRoutes).toBe('appRoutesMock')
    expect(mod.RouteType).toBe('RouteTypeMock')
  })
})
