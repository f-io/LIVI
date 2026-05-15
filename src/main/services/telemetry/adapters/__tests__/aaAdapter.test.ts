import type { AaDriver } from '@projection/driver/aa/aaDriver'
import { TelemetryStore } from '../../TelemetryStore'
import { attachAaAdapter, mapGearToAa } from '../aaAdapter'

function fakeDriver() {
  return {
    sendSpeedData: jest.fn(),
    sendRpmData: jest.fn(),
    sendGearData: jest.fn(),
    sendNightModeData: jest.fn(),
    sendParkingBrakeData: jest.fn(),
    sendDrivingStatusData: jest.fn(),
    sendLightData: jest.fn(),
    sendFuelData: jest.fn(),
    sendOdometerData: jest.fn(),
    sendEnvironmentData: jest.fn(),
    sendGpsLocationData: jest.fn(),
    sendVehicleEnergyModel: jest.fn()
  }
}

function setup() {
  const store = new TelemetryStore()
  const driver = fakeDriver()
  const handle = attachAaAdapter({
    store,
    getAaDriver: () => driver as unknown as AaDriver
  })
  return { store, driver, handle }
}

describe('mapGearToAa', () => {
  test('numeric reverse → 102', () => {
    expect(mapGearToAa(-1, false)).toBe(102)
  })
  test('numeric neutral / drive range', () => {
    expect(mapGearToAa(0, false)).toBe(0)
    expect(mapGearToAa(3, false)).toBe(3)
  })
  test('string P/R/N/D/S', () => {
    expect(mapGearToAa('P', false)).toBe(101)
    expect(mapGearToAa('R', false)).toBe(102)
    expect(mapGearToAa('N', false)).toBe(0)
    expect(mapGearToAa('D', false)).toBe(100)
    expect(mapGearToAa('S', false)).toBe(100)
  })
  test('string M1..M10', () => {
    expect(mapGearToAa('M1', false)).toBe(1)
    expect(mapGearToAa('M10', false)).toBe(10)
  })
  test('reverseFlag alone yields 102', () => {
    expect(mapGearToAa(undefined, true)).toBe(102)
  })
  test('unknown / undefined → undefined', () => {
    expect(mapGearToAa(undefined, undefined)).toBeUndefined()
    expect(mapGearToAa('X', undefined)).toBeUndefined()
  })
})

describe('aaAdapter — single-field forwarders', () => {
  test('speedKph → sendSpeedData in mm/s', () => {
    const { store, driver } = setup()
    store.merge({ speedKph: 36 }) // 10 m/s → 10000 mm/s
    expect(driver.sendSpeedData).toHaveBeenCalledWith(10_000)
  })

  test('idempotent: same speed twice fires only once', () => {
    const { store, driver } = setup()
    store.merge({ speedKph: 36 })
    store.merge({ speedKph: 36 })
    expect(driver.sendSpeedData).toHaveBeenCalledTimes(1)
  })

  test('rpm → sendRpmData × 1000', () => {
    const { store, driver } = setup()
    store.merge({ rpm: 2500 })
    expect(driver.sendRpmData).toHaveBeenCalledWith(2_500_000)
  })

  test('nightMode boolean forwards', () => {
    const { store, driver } = setup()
    store.merge({ nightMode: true })
    expect(driver.sendNightModeData).toHaveBeenCalledWith(true)
  })

  test('parkingBrake boolean forwards', () => {
    const { store, driver } = setup()
    store.merge({ parkingBrake: true })
    expect(driver.sendParkingBrakeData).toHaveBeenCalledWith(true)
  })

  test('drivingStatus integer forwards', () => {
    const { store, driver } = setup()
    store.merge({ drivingStatus: 3 })
    expect(driver.sendDrivingStatusData).toHaveBeenCalledWith(3)
  })

  test('gear → mapGearToAa → sendGearData', () => {
    const { store, driver } = setup()
    store.merge({ gear: 'D' })
    expect(driver.sendGearData).toHaveBeenCalledWith(100)
  })
})

describe('aaAdapter — bundled fields', () => {
  test('lights/highBeam/turn/hazards → one sendLightData call', () => {
    const { store, driver } = setup()
    store.merge({ highBeam: true, turn: 'left', hazards: false })
    expect(driver.sendLightData).toHaveBeenCalledWith(3, false, 2)
  })

  test('lights=true (no high beam) → head=2', () => {
    const { store, driver } = setup()
    store.merge({ lights: true, turn: 'right', hazards: true })
    expect(driver.sendLightData).toHaveBeenCalledWith(2, true, 3)
  })

  test('lights=false → head=1', () => {
    const { store, driver } = setup()
    store.merge({ lights: false, turn: 'none' })
    expect(driver.sendLightData).toHaveBeenCalledWith(1, undefined, 1)
  })

  test('fuelPct + rangeKm → sendFuelData (clamped to 0..100, m, lowFuel bool)', () => {
    const { store, driver } = setup()
    store.merge({ fuelPct: 5, rangeKm: 100 })
    expect(driver.sendFuelData).toHaveBeenCalledWith(5, 100_000, true)
  })

  test('fuelPct >= 10 → lowFuel=false', () => {
    const { store, driver } = setup()
    store.merge({ fuelPct: 50, rangeKm: 400 })
    expect(driver.sendFuelData).toHaveBeenCalledWith(50, 400_000, false)
  })

  test('fuelPct > 100 is clamped to 100', () => {
    const { store, driver } = setup()
    store.merge({ fuelPct: 150 })
    expect(driver.sendFuelData).toHaveBeenCalledWith(100, undefined, false)
  })

  test('odometerKm × 10 + tripKm × 10', () => {
    const { store, driver } = setup()
    store.merge({ odometerKm: 12_345.6, odometerTripKm: 12.3 })
    expect(driver.sendOdometerData).toHaveBeenCalledWith(123_456, 123)
  })

  test('ambientC + baroKpa → sendEnvironmentData', () => {
    const { store, driver } = setup()
    store.merge({ ambientC: 22.5, baroKpa: 101.3 })
    expect(driver.sendEnvironmentData).toHaveBeenCalledWith(22_500, 101_300)
  })

  test('gps fix with full payload forwards each field', () => {
    const { store, driver } = setup()
    store.merge({
      gps: { lat: 52.5, lng: 13.4, accuracyM: 5, alt: 100, speedMs: 10, heading: 90 }
    })
    expect(driver.sendGpsLocationData).toHaveBeenCalledWith({
      latDeg: 52.5,
      lngDeg: 13.4,
      accuracyM: 5,
      altitudeM: 100,
      speedMs: 10,
      bearingDeg: 90
    })
  })

  test('gps without lat/lng is ignored', () => {
    const { store, driver } = setup()
    store.merge({ gps: { lat: 52.5 } })
    expect(driver.sendGpsLocationData).not.toHaveBeenCalled()
  })

  test('vehicle energy model is sent when capacity + range present', () => {
    const { store, driver } = setup()
    store.merge({ batteryCapacityKwh: 50, rangeKm: 200, batteryLevelKwh: 30 })
    expect(driver.sendVehicleEnergyModel).toHaveBeenCalled()
  })

  test('VEM is throttled (10s)', () => {
    const { store, driver } = setup()
    store.merge({ batteryCapacityKwh: 50, rangeKm: 200, batteryLevelKwh: 30 })
    store.merge({ batteryCapacityKwh: 50, rangeKm: 199, batteryLevelKwh: 30 })
    expect(driver.sendVehicleEnergyModel).toHaveBeenCalledTimes(1)
  })

  test('VEM is skipped when range is 0', () => {
    const { store, driver } = setup()
    store.merge({ batteryCapacityKwh: 50, rangeKm: 0, batteryLevelKwh: 30 })
    expect(driver.sendVehicleEnergyModel).not.toHaveBeenCalled()
  })
})

describe('aaAdapter — driver-not-active path', () => {
  test('no calls when getAaDriver returns null', () => {
    const store = new TelemetryStore()
    const driver = fakeDriver()
    attachAaAdapter({ store, getAaDriver: () => null })
    store.merge({ speedKph: 50 })
    expect(driver.sendSpeedData).not.toHaveBeenCalled()
  })
})

describe('aaAdapter — handle', () => {
  test('off detaches the listener', () => {
    const { store, driver, handle } = setup()
    handle.off()
    store.merge({ speedKph: 50 })
    expect(driver.sendSpeedData).not.toHaveBeenCalled()
  })

  test('hydrate replays the current snapshot to a fresh subscription', () => {
    const { store, driver, handle } = setup()
    store.merge({ speedKph: 36 })
    driver.sendSpeedData.mockClear()
    handle.hydrate()
    expect(driver.sendSpeedData).toHaveBeenCalledWith(10_000)
  })

  test('hydrate on an empty store is a no-op', () => {
    const { driver, handle } = setup()
    handle.hydrate()
    expect(driver.sendSpeedData).not.toHaveBeenCalled()
  })
})
