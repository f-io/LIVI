import { encodeNmea } from '../nmea'

describe('encodeNmea hdop', () => {
  test('derives the hdop from the horizontal accuracy', () => {
    const nmea = encodeNmea(52.5, 13.4, 30, 90, 5, 1_700_000_000_000, 10)
    expect(nmea).toContain(',2.0,')
  })

  test('clamps the hdop into 0.5..50', () => {
    expect(encodeNmea(52.5, 13.4, 0, 0, 0, undefined, 1)).toContain(',0.5,')
    expect(encodeNmea(52.5, 13.4, 0, 0, 0, undefined, 10000)).toContain(',50.0,')
  })

  test('falls back to hdop 1.0 for zero or missing accuracy', () => {
    expect(encodeNmea(52.5, 13.4, 0, 0, 0, undefined, 0)).toContain(',1.0,')
    expect(encodeNmea(52.5, 13.4, 0, 0, 0, undefined, undefined)).toContain(',1.0,')
  })
})

describe('encodeNmea optional fields and hemispheres', () => {
  test('falls back to zero altitude, course and speed when they are unset', () => {
    const nmea = encodeNmea(-33.9, -70.6, undefined, undefined, undefined, 1_700_000_000_000)
    // altStr falls back to '0.0', ahead of the GGA body's fixed geoid-separator '0.0,M,,'
    expect(nmea).toContain(',0.0,M,0.0,M,,')
    // speedKn and courseStr both fall back to '0.00' in the RMC body
    expect(nmea).toContain(',0.00,0.00,')
  })

  // Northern latitude / eastern longitude are already covered by the hdop tests above.
  test('renders southern latitude and western longitude hemispheres', () => {
    const nmea = encodeNmea(-33.9, -70.6, 30, 90, 5, 1_700_000_000_000)
    expect(nmea).toContain(',3354.0000,S,')
    expect(nmea).toContain(',07036.0000,W,')
  })
})
