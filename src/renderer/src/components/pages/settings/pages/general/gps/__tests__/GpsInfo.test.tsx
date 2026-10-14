import {
  DEFAULT_CONFIG,
  EMPTY_GNSS_INFO,
  type GnssInfo,
  type GnssSatellite,
  type GpsFix
} from '@shared/types'
import { useLiviStore } from '@store/store'
import { cleanup, render, screen } from '@testing-library/react'
import { GpsInfo } from '../GpsInfo'

const telemetry: { gnss?: GnssInfo; gps?: GpsFix } = {}

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }))
vi.mock('../../../../../telemetry/hooks/useVehicleTelemetry', () => ({
  useVehicleTelemetry: () => ({ telemetry, isStale: false })
}))

function show(gnss?: GnssInfo, gps?: GpsFix) {
  telemetry.gnss = gnss
  telemetry.gps = gps
  return render(<GpsInfo />)
}

const CONNECTED: GnssInfo = {
  ...EMPTY_GNSS_INFO,
  connected: true,
  device: '/dev/ttyAMA0',
  baudRate: 38400,
  fixQuality: 'gps',
  fixMode: '3d',
  satellitesUsed: 9,
  satellitesVisible: 31
}

describe('GpsInfo', () => {
  test('shows a disabled hint when the receiver reports nothing', () => {
    show(undefined)
    expect(screen.getByText('settings.gpsDisabled')).toBeInTheDocument()
  })

  test('reports the connected state and the satellite counts', () => {
    show(CONNECTED)
    expect(screen.getByText('settings.gpsConnected')).toBeInTheDocument()
    expect(screen.getByText('9 / 31')).toBeInTheDocument()
    expect(screen.getByText('3D')).toBeInTheDocument()
  })

  test('surfaces the error instead of a bare disconnected label', () => {
    show({ ...EMPTY_GNSS_INFO, error: '/dev/ttyAMA0 not found' })
    expect(screen.getByText('/dev/ttyAMA0 not found')).toBeInTheDocument()
  })

  test('falls back to a disconnected label without an error', () => {
    show({ ...EMPTY_GNSS_INFO })
    expect(screen.getByText('settings.gpsDisconnected')).toBeInTheDocument()
  })

  test('leaves out device and baud rate, which live in the settings', () => {
    show(CONNECTED)
    expect(screen.queryByText('/dev/ttyAMA0')).not.toBeInTheDocument()
    expect(screen.queryByText('38400')).not.toBeInTheDocument()
  })

  test('shows the receiver clock and the derived zone', () => {
    show({
      ...CONNECTED,
      receiverTime: Date.UTC(2026, 7, 19, 14, 47, 10),
      timezone: 'Europe/Berlin'
    })
    expect(screen.getByText('2026-08-19 14:47:10 UTC')).toBeInTheDocument()
    expect(screen.getByText('Europe/Berlin')).toBeInTheDocument()
  })

  test('gives every constellation its own row', () => {
    show({
      ...CONNECTED,
      satellites: [
        { id: 1, constellation: 'gps', used: true, snr: 40 },
        { id: 2, constellation: 'gps', used: false, snr: 20 },
        { id: 3, constellation: 'galileo', used: true, snr: 44 },
        { id: 4, constellation: 'glonass', used: false },
        { id: 5, constellation: 'beidou', used: false },
        { id: 6, constellation: 'qzss', used: false },
        { id: 7, constellation: 'unknown', used: false }
      ]
    })
    expect(screen.getByText('GPS')).toBeInTheDocument()
    expect(screen.getByText('1 / 2')).toBeInTheDocument()
    expect(screen.getByText('Galileo')).toBeInTheDocument()
    expect(screen.getByText('GLONASS')).toBeInTheDocument()
    expect(screen.getByText('BeiDou')).toBeInTheDocument()
    expect(screen.getByText('QZSS')).toBeInTheDocument()
  })

  test('averages the four strongest satellites', () => {
    show({
      ...CONNECTED,
      satellites: [
        { id: 1, constellation: 'gps', used: true, snr: 40 },
        { id: 2, constellation: 'gps', used: true, snr: 44 },
        { id: 3, constellation: 'gps', used: false, snr: 50 },
        { id: 4, constellation: 'gps', used: false, snr: 38 },
        { id: 5, constellation: 'gps', used: false, snr: 30 }
      ]
    })
    // (50 + 44 + 40 + 38) / 4 — the 30 at the horizon stays out
    expect(screen.getByText('43 dB-Hz · settings.gpsRatingGood')).toBeInTheDocument()
  })

  test('a weak satellite joining does not drag the level down', () => {
    const strong: GnssSatellite[] = [
      { id: 1, constellation: 'gps', used: true, snr: 46 },
      { id: 2, constellation: 'gps', used: true, snr: 46 },
      { id: 3, constellation: 'gps', used: true, snr: 46 },
      { id: 4, constellation: 'gps', used: true, snr: 46 }
    ]
    show({ ...CONNECTED, satellites: strong })
    expect(screen.getByText('46 dB-Hz · settings.gpsRatingExcellent')).toBeInTheDocument()

    cleanup()
    show({
      ...CONNECTED,
      satellites: [...strong, { id: 9, constellation: 'gps', used: false, snr: 8 }]
    })
    expect(screen.getByText('46 dB-Hz · settings.gpsRatingExcellent')).toBeInTheDocument()
  })

  test('averages what it has while fewer than four are heard', () => {
    show({
      ...CONNECTED,
      satellites: [
        { id: 1, constellation: 'gps', used: false, snr: 40 },
        { id: 2, constellation: 'gps', used: false, snr: 36 }
      ]
    })
    expect(screen.getByText('38 dB-Hz · settings.gpsRatingGood')).toBeInTheDocument()
  })

  test('omits the signal row when no satellite reports a level', () => {
    show({ ...CONNECTED, satellites: [{ id: 1, constellation: 'gps', used: true }] })
    expect(screen.queryByText('settings.gpsSignal')).not.toBeInTheDocument()
  })

  test('renders the DOP triplet, filling gaps with a dash', () => {
    show({ ...CONNECTED, hdop: 1.5, pdop: 2.5 })
    expect(screen.getByText('2.5 / 1.5 / –')).toBeInTheDocument()
  })

  test('omits the DOP row without an HDOP', () => {
    show({ ...CONNECTED, pdop: 2.5 })
    expect(screen.queryByText(/2\.5 \//)).not.toBeInTheDocument()
  })

  test('renders position, altitude, speed and accuracy from the fix', () => {
    show(CONNECTED, { lat: 53.353555, lng: 10.563336, alt: 28.2, speedMs: 10, accuracyM: 2.5 })
    expect(screen.getByText('53.353555, 10.563336')).toBeInTheDocument()
    expect(screen.getByText('28.2 m')).toBeInTheDocument()
    expect(screen.getByText('36.0 km/h')).toBeInTheDocument()
    expect(screen.getByText('± 2.5 m')).toBeInTheDocument()
  })

  test('shows the speed in mph when asked', () => {
    useLiviStore.setState({ settings: { ...DEFAULT_CONFIG, speedUnit: 'mph' } })
    try {
      show(CONNECTED, { lat: 53.3, lng: 10.5, speedMs: 10 })
      expect(screen.getByText('22.4 mph')).toBeInTheDocument()
    } finally {
      useLiviStore.setState({ settings: null })
    }
  })

  test('omits the position row when the fix carries no coordinates', () => {
    show(CONNECTED, { alt: 10 })
    expect(screen.queryByText('settings.gpsPosition')).not.toBeInTheDocument()
  })

  test('maps an unknown fix quality to an empty label', () => {
    show({ ...CONNECTED, fixQuality: 'weird' as never })
    expect(screen.getByText('settings.gpsFixMode')).toBeInTheDocument()
  })

  test('shows the no-fix label for quality and mode without a lock', () => {
    show({ ...CONNECTED, fixQuality: 'none', fixMode: 'none' })
    expect(screen.getAllByText('settings.gpsFixNone')).toHaveLength(2)
  })

  test('shows the mode once there is a fix', () => {
    show({ ...CONNECTED, fixMode: '2d' })
    expect(screen.getByText('2D')).toBeInTheDocument()
  })

  test.each([
    [48, 'settings.gpsRatingExcellent'],
    [40, 'settings.gpsRatingGood'],
    [34, 'settings.gpsRatingWeak'],
    [21, 'settings.gpsRatingPoor']
  ])('rates a signal of %i dB-Hz', (snr, key) => {
    show({ ...CONNECTED, satellites: [{ id: 1, constellation: 'gps', used: true, snr }] })
    expect(screen.getByText(new RegExp(key))).toBeInTheDocument()
  })

  test('shows the DOP numbers without a quality word, unlike the signal', () => {
    show({ ...CONNECTED, hdop: 1.3, pdop: 6, vdop: 1.7 })
    expect(screen.getByText('6.0 / 1.3 / 1.7')).toBeInTheDocument()
  })

  test('hides the DOP row while NMEA only sends its placeholder', () => {
    show({ ...CONNECTED, hdop: 99.99, pdop: 99.99, vdop: 99.99 })
    expect(screen.queryByText('settings.gpsDop')).not.toBeInTheDocument()
  })

  test('keeps the DOP row across a brief fix dropout', () => {
    show({ ...CONNECTED, fixMode: 'none', hdop: 1.3, pdop: 6, vdop: 1.7 })
    expect(screen.getByText('6.0 / 1.3 / 1.7')).toBeInTheDocument()
  })

  test('shows the signal while still searching, before any fix', () => {
    show({
      ...EMPTY_GNSS_INFO,
      connected: true,
      satellites: [
        { id: 1, constellation: 'gps', used: false, snr: 24 },
        { id: 2, constellation: 'gps', used: false, snr: 22 }
      ]
    })
    expect(screen.getByText('23 dB-Hz · settings.gpsRatingPoor')).toBeInTheDocument()
  })

  test('keeps a reported system listed at 0 / 0 when it drops out', () => {
    show({
      ...CONNECTED,
      constellations: ['gps', 'glonass', 'galileo'],
      satellites: [{ id: 1, constellation: 'gps', used: true, snr: 40 }]
    })
    expect(screen.getByText('GPS')).toBeInTheDocument()
    expect(screen.getByText('GLONASS')).toBeInTheDocument()
    expect(screen.getByText('Galileo')).toBeInTheDocument()
    expect(screen.getAllByText('0 / 0')).toHaveLength(2)
  })

  test('lists no system the receiver never reported', () => {
    show({
      ...CONNECTED,
      constellations: ['gps'],
      satellites: [{ id: 1, constellation: 'gps', used: true, snr: 40 }]
    })
    expect(screen.queryByText('BeiDou')).not.toBeInTheDocument()
    expect(screen.queryByText('QZSS')).not.toBeInTheDocument()
  })
})
