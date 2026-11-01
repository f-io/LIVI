import { decodeHaltechCanLine } from '../haltechCanDecoder'

describe('decodeHaltechCanLine', () => {
  test('decodes Haltech engine RPM as an unsigned big-endian value', () => {
    expect(decodeHaltechCanLine('(0000000001.000000) can0 360#1388000000000000')).toEqual({
      rpm: 5000
    })
  })

  test('decodes Haltech wheel speed in tenths of km/h', () => {
    expect(decodeHaltechCanLine('can0 370#03E8000000000000')).toEqual({ speedKph: 100 })
  })

  test('ignores unrelated, malformed, extended, and short frames', () => {
    expect(decodeHaltechCanLine('can0 361#0000000000000000')).toBeNull()
    expect(decodeHaltechCanLine('can0 1800#0000')).toBeNull()
    expect(decodeHaltechCanLine('can0 360#0')).toBeNull()
    expect(decodeHaltechCanLine('can0 370#')).toBeNull()
    expect(decodeHaltechCanLine('can1 360#1388')).toBeNull()
  })
})
