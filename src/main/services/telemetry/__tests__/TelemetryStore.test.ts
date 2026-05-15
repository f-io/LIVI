import { TelemetryStore } from '../TelemetryStore'

describe('TelemetryStore', () => {
  test('initial snapshot is empty', () => {
    const s = new TelemetryStore()
    expect(s.snapshot()).toEqual({})
  })

  test('merge stores fields + emits change with patch and snapshot', () => {
    const s = new TelemetryStore()
    const cb = jest.fn()
    s.on('change', cb)
    s.merge({ speedKph: 50 })
    expect(cb).toHaveBeenCalledTimes(1)
    const [patch, snap] = cb.mock.calls[0]
    expect(patch.speedKph).toBe(50)
    expect(snap.speedKph).toBe(50)
  })

  test('snapshot returns a fresh copy each call', () => {
    const s = new TelemetryStore()
    s.merge({ speedKph: 10 })
    const a = s.snapshot()
    const b = s.snapshot()
    expect(a).not.toBe(b)
    a.speedKph = 999
    expect(s.snapshot().speedKph).toBe(10)
  })

  test('merge with null / empty / non-object is a no-op', () => {
    const s = new TelemetryStore()
    const cb = jest.fn()
    s.on('change', cb)
    s.merge(null)
    s.merge(undefined)
    s.merge({})
    s.merge([] as never)
    expect(cb).not.toHaveBeenCalled()
  })

  test('undefined values in patch are skipped', () => {
    const s = new TelemetryStore()
    s.merge({ speedKph: 50, rpm: undefined })
    expect(s.snapshot().speedKph).toBe(50)
    expect('rpm' in s.snapshot()).toBe(false)
  })

  test('nested gps block merges field-by-field', () => {
    const s = new TelemetryStore()
    s.merge({ gps: { lat: 52, lon: 13 } })
    s.merge({ gps: { lon: 14 } })
    expect(s.snapshot().gps).toEqual({ lat: 52, lon: 14 })
  })

  test('non-nested fields use replace-on-write', () => {
    const s = new TelemetryStore()
    s.merge({ speedKph: 50 })
    s.merge({ speedKph: 60 })
    expect(s.snapshot().speedKph).toBe(60)
  })

  test('merged patch event re-emits nested keys as their merged form', () => {
    const s = new TelemetryStore()
    s.merge({ gps: { lat: 52 } })
    const cb = jest.fn()
    s.on('change', cb)
    s.merge({ gps: { lon: 13 } })
    const [patch] = cb.mock.calls[0]
    expect(patch.gps).toEqual({ lat: 52, lon: 13 })
  })

  test('ts is filled in when missing', () => {
    const s = new TelemetryStore()
    const cb = jest.fn()
    s.on('change', cb)
    s.merge({ speedKph: 10 })
    expect(typeof cb.mock.calls[0][0].ts).toBe('number')
  })

  test('ts from the patch is preserved when supplied', () => {
    const s = new TelemetryStore()
    s.merge({ speedKph: 10, ts: 12345 })
    expect(s.snapshot().ts).toBe(12345)
  })

  test('reset clears the snapshot', () => {
    const s = new TelemetryStore()
    s.merge({ speedKph: 10 })
    s.reset()
    expect(s.snapshot()).toEqual({})
  })

  test('off removes a listener', () => {
    const s = new TelemetryStore()
    const cb = jest.fn()
    s.on('change', cb)
    s.off('change', cb)
    s.merge({ speedKph: 1 })
    expect(cb).not.toHaveBeenCalled()
  })
})
