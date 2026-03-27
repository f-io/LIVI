jest.mock('../../components/pages', () => ({
  Home: () => null,
  Media: () => null,
  Camera: () => null,
  Maps: () => null,
  Telemetry: () => null
}))
jest.mock('../../components/layouts/Layout', () => ({
  Layout: () => null
}))
jest.mock('../../components/pages/settings/SettingsPage', () => ({
  SettingsPage: () => null
}))
jest.mock('../schemas.ts/schema', () => ({
  settingsRoutes: { children: [{ path: 'general' }] }
}))

import { appRoutes } from '../appRoutes'

describe('appRoutes', () => {
  test('contains expected top-level app routes', () => {
    const root = appRoutes[0]
    const paths = (root.children ?? []).map((r: any) => r.path)
    expect(paths).toEqual(['/home', '/telemetry', '/maps', '/media', '/camera', '/settings/*'])
  })

  test('falls back to empty settings children when settingsRoutes is missing', () => {
    jest.resetModules()

    jest.isolateModules(() => {
      jest.doMock('../../components/pages', () => ({
        Home: () => null,
        Media: () => null,
        Camera: () => null,
        Maps: () => null,
        Telemetry: () => null
      }))
      jest.doMock('../../components/layouts/Layout', () => ({
        Layout: () => null
      }))
      jest.doMock('../../components/pages/settings/SettingsPage', () => ({
        SettingsPage: () => null
      }))
      jest.doMock('../schemas.ts/schema', () => ({
        settingsRoutes: undefined
      }))

      const { appRoutes: isolatedAppRoutes } = require('../appRoutes')
      const root = isolatedAppRoutes[0]
      const settingsRoute = root.children.find((r: any) => r.path === '/settings/*')

      expect(settingsRoute.children).toEqual([])
    })
  })
})
