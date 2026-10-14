import { clusterTargetScreens, isClusterOnScreen } from '@main/shared/utils'

describe('cluster utils', () => {
  test('clusterTargetScreens collects roles enabled on dashboard3 or dashboard4', () => {
    expect(
      clusterTargetScreens({
        dashboards: { dashboard3: { main: true, cluster: false }, dashboard4: { aux: true } }
      })
    ).toEqual(['main', 'aux'])
  })

  test('clusterTargetScreens returns empty list for missing config', () => {
    expect(clusterTargetScreens(null)).toEqual([])
    expect(clusterTargetScreens(undefined)).toEqual([])
    expect(clusterTargetScreens({ dashboards: null })).toEqual([])
  })

  test('isClusterOnScreen checks a single role', () => {
    const cfg = { dashboards: { dashboard3: { main: true, cluster: false, aux: false } } }
    expect(isClusterOnScreen(cfg, 'main')).toBe(true)
    expect(isClusterOnScreen(cfg, 'cluster')).toBe(false)
    expect(isClusterOnScreen(null, 'aux')).toBe(false)
  })
})
