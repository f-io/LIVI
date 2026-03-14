jest.mock('@renderer/components/pages/settings/SettingsPage', () => ({
  SettingsPage: () => null
}))

import { generateRoutes } from '../generateRoutes'

describe('generateRoutes', () => {
  test('returns null for non-route nodes', () => {
    const node = { type: 'checkbox', path: 'x', label: 'X' } as any
    expect(generateRoutes(node)).toBeNull()
  })

  test('creates RouteObject tree for route nodes', () => {
    const root = {
      type: 'route',
      route: 'settings',
      label: 'Settings',
      path: '',
      children: [
        { type: 'checkbox', path: 'mute', label: 'Mute' },
        { type: 'route', route: 'audio', label: 'Audio', path: '', children: [] }
      ]
    } as any

    const route = generateRoutes(root)
    expect(route?.path).toBe('settings')
    expect(route?.children).toHaveLength(1)
    expect(route?.children?.[0]?.path).toBe('audio')
  })
})
