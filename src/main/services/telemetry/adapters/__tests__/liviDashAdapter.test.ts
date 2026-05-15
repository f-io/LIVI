import type { WebContents } from 'electron'
import { TelemetryStore } from '../../TelemetryStore'
import { attachLiviDashAdapter } from '../liviDashAdapter'

function fakeWc(destroyed = false) {
  return {
    send: jest.fn(),
    isDestroyed: jest.fn(() => destroyed)
  } as unknown as WebContents & { send: jest.Mock; isDestroyed: jest.Mock }
}

beforeEach(() => {
  jest.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('liviDashAdapter', () => {
  test('forwards the snapshot to a single webContents', () => {
    const store = new TelemetryStore()
    const wc = fakeWc()
    attachLiviDashAdapter({ store, getWebContents: () => wc })
    store.merge({ speedKph: 42 })
    expect(wc.send).toHaveBeenCalledWith(
      'telemetry:update',
      expect.objectContaining({ speedKph: 42 })
    )
  })

  test('forwards to every webContents in an array', () => {
    const store = new TelemetryStore()
    const wcA = fakeWc()
    const wcB = fakeWc()
    attachLiviDashAdapter({ store, getWebContents: () => [wcA, wcB] })
    store.merge({ speedKph: 10 })
    expect(wcA.send).toHaveBeenCalled()
    expect(wcB.send).toHaveBeenCalled()
  })

  test('skips destroyed webContents', () => {
    const store = new TelemetryStore()
    const wc = fakeWc(true)
    attachLiviDashAdapter({ store, getWebContents: () => wc })
    store.merge({ speedKph: 1 })
    expect(wc.send).not.toHaveBeenCalled()
  })

  test('no-op when getWebContents returns null', () => {
    const store = new TelemetryStore()
    attachLiviDashAdapter({ store, getWebContents: () => null })
    expect(() => store.merge({ speedKph: 5 })).not.toThrow()
  })

  test('returned function detaches the listener', () => {
    const store = new TelemetryStore()
    const wc = fakeWc()
    const off = attachLiviDashAdapter({ store, getWebContents: () => wc })
    off()
    store.merge({ speedKph: 99 })
    expect(wc.send).not.toHaveBeenCalled()
  })

  test('a thrown send is swallowed and warned', () => {
    const store = new TelemetryStore()
    const wc = fakeWc()
    wc.send.mockImplementation(() => {
      throw new Error('detached')
    })
    attachLiviDashAdapter({ store, getWebContents: () => wc })
    expect(() => store.merge({ speedKph: 1 })).not.toThrow()
  })
})
