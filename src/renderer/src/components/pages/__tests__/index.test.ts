vi.mock('../camera', () => ({
  __esModule: true,
  Camera: 'CameraMock'
}))

vi.mock('../projection', () => ({
  __esModule: true,
  Projection: 'ProjectionMock'
}))

vi.mock('../cluster', () => ({
  __esModule: true,
  Cluster: 'ClusterMock'
}))

vi.mock('../media', () => ({
  __esModule: true,
  Media: 'MediaMock'
}))

vi.mock('../settings', () => ({
  __esModule: true,
  SettingsPage: 'SettingsPageMock'
}))

vi.mock('../telemetry', () => ({
  __esModule: true,
  Telemetry: 'TelemetryMock'
}))

describe('pages index', () => {
  test('re-exports page modules', async () => {
    const mod = await import('../index')

    expect(mod.Camera).toBe('CameraMock')
    expect(mod.Projection).toBe('ProjectionMock')
    expect(mod.Cluster).toBe('ClusterMock')
    expect(mod.Media).toBe('MediaMock')
    expect(mod.SettingsPage).toBe('SettingsPageMock')
    expect(mod.Telemetry).toBe('TelemetryMock')
  })
})
