import { getNodeByPath, getValueByPath, setValueByPath } from '../utils'

describe('settings utils', () => {
  test('getValueByPath returns nested value and undefined for missing paths', () => {
    const obj = { a: { b: { c: 7 } } }
    expect(getValueByPath(obj, 'a.b.c')).toBe(7)
    expect(getValueByPath(obj, 'a.b.x')).toBeUndefined()
    expect(getValueByPath(obj, '')).toBeUndefined()
  })

  test('setValueByPath creates nested records when needed', () => {
    const obj: Record<string, unknown> = {}
    setValueByPath(obj, 'ui.theme.primary', '#fff')
    expect(obj).toEqual({ ui: { theme: { primary: '#fff' } } })
  })

  test('getNodeByPath resolves route chains and leaf nodes', () => {
    const tree = {
      type: 'route',
      route: 'settings',
      path: 'settings',
      label: 'Settings',
      children: [
        {
          type: 'route',
          route: 'audio',
          path: 'audio',
          label: 'Audio',
          children: [{ type: 'checkbox', path: 'mute', label: 'Mute' }]
        }
      ]
    } as any

    const leaf = getNodeByPath(tree, ['audio', 'mute'])
    expect(leaf && 'path' in leaf ? leaf.path : null).toBe('mute')

    const route = getNodeByPath(tree, ['audio'])
    expect(route && route.type).toBe('route')

    expect(getNodeByPath(tree, ['video'])).toBeNull()
  })
})
