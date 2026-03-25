jest.mock('../dash1/Dash1', () => ({
  Dash1: 'Dash1'
}))

jest.mock('../dash2/Dash2', () => ({
  Dash2: 'Dash2'
}))

jest.mock('../dash3/Dash3', () => ({
  Dash3: 'Dash3'
}))

jest.mock('../dash4/Dash4', () => ({
  Dash4: 'Dash4'
}))

describe('telemetry dashboards index', () => {
  test('re-exports all dashboard modules', () => {
    const mod = require('../index')

    expect(mod.Dash1).toBe('Dash1')
    expect(mod.Dash2).toBe('Dash2')
    expect(mod.Dash3).toBe('Dash3')
    expect(mod.Dash4).toBe('Dash4')
  })
})
