jest.mock('../Cluster', () => ({
  __esModule: true,
  Cluster: 'ClusterMock'
}))

describe('cluster index', () => {
  test('re-exports Cluster module', () => {
    const mod = require('../index')

    expect(mod.Cluster).toBe('ClusterMock')
  })
})
