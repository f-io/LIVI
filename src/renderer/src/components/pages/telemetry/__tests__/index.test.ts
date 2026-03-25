jest.mock('../Telemetry', () => ({
  __esModule: true,
  Telemetry: 'TelemetryMock'
}))

describe('telemetry index', () => {
  test('re-exports Telemetry module', () => {
    const mod = require('../index')

    expect(mod.Telemetry).toBe('TelemetryMock')
  })
})
