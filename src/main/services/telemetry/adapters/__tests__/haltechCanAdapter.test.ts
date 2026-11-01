import type { ChildProcess } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import { TelemetryStore } from '../../TelemetryStore'
import { attachHaltechCanAdapter } from '../haltechCanAdapter'

function captureProcess() {
  return Object.assign(new EventEmitter(), {
    stdout: new PassThrough(),
    stderr: new PassThrough(),
    kill: vi.fn(() => true)
  }) as unknown as ChildProcess
}

describe('haltechCanAdapter', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  test('starts only when enabled, decodes telemetry, and stops on disable', () => {
    const store = new TelemetryStore()
    const capture = captureProcess()
    const spawnCapture = vi.fn(() => capture)
    const adapter = attachHaltechCanAdapter({ store, enabled: false, spawnCapture })

    expect(spawnCapture).not.toHaveBeenCalled()
    adapter.setEnabled(true)
    expect(spawnCapture).toHaveBeenCalledTimes(1)

    capture.stdout?.write('can0 360#1388000000000000\ncan0 370#03E8000000000000\n')
    expect(store.snapshot()).toMatchObject({ rpm: 5000, speedKph: 100 })

    adapter.setEnabled(false)
    expect(capture.kill).toHaveBeenCalledWith('SIGTERM')
    expect(store.snapshot()).toMatchObject({ rpm: 0, speedKph: 0 })
    adapter.off()
  })

  test('clears values when their CAN frames become stale', () => {
    vi.useFakeTimers()
    const store = new TelemetryStore()
    const capture = captureProcess()
    const adapter = attachHaltechCanAdapter({
      store,
      enabled: true,
      spawnCapture: () => capture
    })

    capture.stdout?.write('can0 360#1388000000000000\n')
    vi.advanceTimersByTime(2000)
    expect(store.snapshot().rpm).toBe(0)
    expect(store.snapshot().speedKph).toBeUndefined()
    adapter.off()
  })
})
