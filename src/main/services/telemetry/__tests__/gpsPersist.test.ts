const requestSaveMock = jest.fn()
jest.mock('@main/ipc/utils', () => ({
  configEvents: { emit: (...args: unknown[]) => requestSaveMock(...args) }
}))

import { attachGpsPersist } from '../gpsPersist'
import { TelemetryStore } from '../TelemetryStore'

beforeEach(() => {
  requestSaveMock.mockReset()
  jest.useFakeTimers()
  jest.setSystemTime(new Date('2026-01-15T12:00:00Z'))
})
afterEach(() => {
  jest.useRealTimers()
})

describe('gpsPersist — hydration', () => {
  test('hydrates the store from a valid initial fix', () => {
    const store = new TelemetryStore()
    attachGpsPersist({
      store,
      initialGps: { lat: 52, lng: 13, ts: 1_700_000_000 }
    })
    expect(store.snapshot().gps).toMatchObject({ lat: 52, lng: 13 })
  })

  test('ignores invalid initial fix (lat=0,lng=0)', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store, initialGps: { lat: 0, lng: 0, ts: 1_700_000_000 } })
    expect(store.snapshot().gps).toBeUndefined()
  })

  test('ignores initial fix without ts', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store, initialGps: { lat: 52, lng: 13 } as never })
    expect(store.snapshot().gps).toBeUndefined()
  })

  test('ignores out-of-range lat/lng', () => {
    const store = new TelemetryStore()
    attachGpsPersist({
      store,
      initialGps: { lat: 99, lng: 200, ts: 1_700_000_000 }
    })
    expect(store.snapshot().gps).toBeUndefined()
  })
})

describe('gpsPersist — persist behavior', () => {
  test('writes immediately when no previous fix exists', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13 } })
    expect(requestSaveMock).toHaveBeenCalledWith(
      'requestSave',
      expect.objectContaining({ lastKnownGps: expect.objectContaining({ lat: 52, lng: 13 }) })
    )
  })

  test('subsequent writes are throttled to 30s', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13 } })
    requestSaveMock.mockClear()
    store.merge({ gps: { lat: 52.001, lng: 13.001 } })
    expect(requestSaveMock).not.toHaveBeenCalled()
  })

  test('throttled write flushes after the interval', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13 } })
    requestSaveMock.mockClear()
    store.merge({ gps: { lat: 53, lng: 14 } })
    jest.advanceTimersByTime(30_000)
    expect(requestSaveMock).toHaveBeenCalled()
  })

  test('does not write when location is unchanged', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13 } })
    requestSaveMock.mockClear()
    jest.advanceTimersByTime(40_000)
    store.merge({ gps: { lat: 52, lng: 13 } })
    expect(requestSaveMock).not.toHaveBeenCalled()
  })

  test('persists alt + heading when supplied', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13, alt: 100, heading: 90 } })
    expect(requestSaveMock.mock.calls[0][1].lastKnownGps).toMatchObject({ alt: 100, heading: 90 })
  })

  test('rejects 0/0 fix', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 0, lng: 0 } })
    expect(requestSaveMock).not.toHaveBeenCalled()
  })

  test('rejects invalid lat/lng', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 91, lng: 0 } })
    store.merge({ gps: { lat: 0, lng: -181 } })
    expect(requestSaveMock).not.toHaveBeenCalled()
  })

  test('off detaches and cancels pending timer', () => {
    const store = new TelemetryStore()
    const handle = attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13 } })
    store.merge({ gps: { lat: 53, lng: 14 } })
    handle.off()
    jest.advanceTimersByTime(60_000)
    // Only the immediate first write
    expect(requestSaveMock).toHaveBeenCalledTimes(1)
  })

  test('pending timer is reused: second push before the first flushes is folded in', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 52, lng: 13 } })
    requestSaveMock.mockClear()
    store.merge({ gps: { lat: 53, lng: 14 } })
    // Second push during the same pending window — should not start a new timer
    store.merge({ gps: { lat: 54, lng: 15 } })
    jest.advanceTimersByTime(30_000)
    // Only one save fires (with the latest snapshot)
    expect(requestSaveMock).toHaveBeenCalledTimes(1)
  })

  test('rejects non-number lat/lng types', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: 'x' as never, lng: 13 as never } })
    expect(requestSaveMock).not.toHaveBeenCalled()
  })

  test('rejects NaN/Infinity', () => {
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    store.merge({ gps: { lat: NaN, lng: 0 } })
    store.merge({ gps: { lat: 0, lng: Infinity } })
    expect(requestSaveMock).not.toHaveBeenCalled()
  })

  test('persist errors are caught', () => {
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    requestSaveMock.mockImplementationOnce(() => {
      throw new Error('IPC down')
    })
    const store = new TelemetryStore()
    attachGpsPersist({ store })
    expect(() => store.merge({ gps: { lat: 52, lng: 13 } })).not.toThrow()
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })
})
