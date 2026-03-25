describe('telemetry widgets index', () => {
  test('re-exports all widgets', () => {
    const mod = require('../index')

    expect(mod).toHaveProperty('CoolantTemp')
    expect(mod).toHaveProperty('FuelLevel')
    expect(mod).toHaveProperty('Gear')
    expect(mod).toHaveProperty('NavFull')
    expect(mod).toHaveProperty('NavMini')
    expect(mod).toHaveProperty('OilTemp')
    expect(mod).toHaveProperty('Rpm')
    expect(mod).toHaveProperty('RpmRing')
    expect(mod).toHaveProperty('Speed')
  })
})
