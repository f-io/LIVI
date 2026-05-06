jest.mock('../camera', () => ({
  __esModule: true,
  Camera: 'CameraMock'
}))

jest.mock('../projection', () => ({
  __esModule: true,
  Projection: 'ProjectionMock'
}))

jest.mock('../home', () => ({
  __esModule: true,
  Home: 'HomeMock'
}))

jest.mock('../cluster', () => ({
  __esModule: true,
  Cluster: 'ClusterMock'
}))

jest.mock('../media', () => ({
  __esModule: true,
  Media: 'MediaMock'
}))

jest.mock('../settings', () => ({
  __esModule: true,
  SettingsPage: 'SettingsPageMock'
}))

jest.mock('../telemetry', () => ({
  __esModule: true,
  Telemetry: 'TelemetryMock'
}))

describe('pages index', () => {
  test('re-exports page modules', () => {
    const mod = require('../index')

    expect(mod.Camera).toBe('CameraMock')
    expect(mod.Projection).toBe('ProjectionMock')
    expect(mod.Home).toBe('HomeMock')
    expect(mod.Cluster).toBe('ClusterMock')
    expect(mod.Media).toBe('MediaMock')
    expect(mod.SettingsPage).toBe('SettingsPageMock')
    expect(mod.Telemetry).toBe('TelemetryMock')
  })
})
