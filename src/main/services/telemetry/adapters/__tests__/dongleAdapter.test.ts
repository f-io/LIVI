import type { DongleDriver } from '@projection/driver/dongle/dongleDriver'
import { SendBoolean } from '@projection/messages/sendable'
import { TelemetryStore } from '../../TelemetryStore'
import { attachDongleAdapter } from '../dongleAdapter'

function fakeDriver() {
  return {
    send: jest.fn(async () => true),
    sendGnssData: jest.fn(async () => true)
  }
}

function setup() {
  const store = new TelemetryStore()
  const driver = fakeDriver()
  const handle = attachDongleAdapter({
    store,
    getDongleDriver: () => driver as unknown as DongleDriver
  })
  return { store, driver, handle }
}

describe('dongleAdapter — nightMode forwarder', () => {
  test('sends a SendBoolean when nightMode toggles', () => {
    const { store, driver } = setup()
    store.merge({ nightMode: true })
    expect(driver.send).toHaveBeenCalledTimes(1)
    const arg = driver.send.mock.calls[0][0]
    expect(arg).toBeInstanceOf(SendBoolean)
  })

  test('does not re-send when value is unchanged', () => {
    const { store, driver } = setup()
    store.merge({ nightMode: true })
    store.merge({ nightMode: true })
    expect(driver.send).toHaveBeenCalledTimes(1)
  })

  test('re-sends when value flips', () => {
    const { store, driver } = setup()
    store.merge({ nightMode: true })
    store.merge({ nightMode: false })
    expect(driver.send).toHaveBeenCalledTimes(2)
  })
})

describe('dongleAdapter — GPS / NMEA', () => {
  test('lat+lng forwards a NMEA string with GPGGA + GPRMC sentences', () => {
    const { store, driver } = setup()
    store.merge({ gps: { lat: 52.5, lng: 13.4 } })
    expect(driver.sendGnssData).toHaveBeenCalledTimes(1)
    const nmea = driver.sendGnssData.mock.calls[0][0] as string
    expect(nmea).toContain('$GPGGA')
    expect(nmea).toContain('$GPRMC')
    expect(nmea).toContain('N')
    expect(nmea).toContain('E')
  })

  test('southern / western hemisphere flips N/E to S/W', () => {
    const { store, driver } = setup()
    store.merge({ gps: { lat: -33, lng: -70 } })
    const nmea = driver.sendGnssData.mock.calls[0][0] as string
    expect(nmea).toContain(',S,')
    expect(nmea).toContain(',W,')
  })

  test('alt/heading/speed/fixTs are encoded when present', () => {
    const { store, driver } = setup()
    store.merge({
      gps: {
        lat: 52,
        lng: 13,
        alt: 100,
        heading: 90,
        speedMs: 10,
        fixTs: Date.UTC(2026, 0, 15, 12, 30, 45)
      }
    })
    const nmea = driver.sendGnssData.mock.calls[0][0] as string
    expect(nmea).toContain('123045.00') // UTC time hhmmss
    expect(nmea).toContain('150126') // ddmmyy
  })

  test('checksum is two hex chars after the asterisk', () => {
    const { store, driver } = setup()
    store.merge({ gps: { lat: 0, lng: 0 } })
    const nmea = driver.sendGnssData.mock.calls[0][0] as string
    expect(/\*[0-9A-F]{2}/.test(nmea)).toBe(true)
  })

  test('gps without lat/lng is ignored', () => {
    const { store, driver } = setup()
    store.merge({ gps: { lat: 52 } })
    expect(driver.sendGnssData).not.toHaveBeenCalled()
  })
})

describe('dongleAdapter — handle', () => {
  test('no calls when getDongleDriver returns null', () => {
    const store = new TelemetryStore()
    const driver = fakeDriver()
    attachDongleAdapter({ store, getDongleDriver: () => null })
    store.merge({ nightMode: true })
    expect(driver.send).not.toHaveBeenCalled()
  })

  test('off stops further forwarding', () => {
    const { store, driver, handle } = setup()
    handle.off()
    store.merge({ nightMode: true })
    expect(driver.send).not.toHaveBeenCalled()
  })

  test('hydrate replays the current snapshot', () => {
    const { store, driver, handle } = setup()
    store.merge({ nightMode: true, gps: { lat: 52, lng: 13 } })
    driver.send.mockClear()
    driver.sendGnssData.mockClear()
    handle.hydrate()
    expect(driver.send).toHaveBeenCalled()
    expect(driver.sendGnssData).toHaveBeenCalled()
  })

  test('hydrate on an empty store is a no-op', () => {
    const { driver, handle } = setup()
    handle.hydrate()
    expect(driver.send).not.toHaveBeenCalled()
    expect(driver.sendGnssData).not.toHaveBeenCalled()
  })
})
