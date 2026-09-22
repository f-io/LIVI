vi.mock('../HostAudioOutput', async () => {
  const instances: Array<{
    started: boolean
    stopped: boolean
    writes: Int16Array[]
    opts: unknown
  }> = []
  return {
    __instances: instances,
    HostAudioOutput: class {
      started = false
      stopped = false
      writes: Int16Array[] = []
      constructor(public opts: unknown) {
        instances.push(this)
      }
      start(): void {
        this.started = true
      }
      write(buf: Int16Array): void {
        this.writes.push(buf)
      }
      stop(): void {
        this.stopped = true
      }
      dispose(): void {}
    }
  }
})

import * as AO from '../HostAudioOutput'
import { renderRelayClick, SystemSound } from '../SystemSound'

type MockOut = {
  started: boolean
  stopped: boolean
  writes: Int16Array[]
  opts: { device?: string }
}
const instances = (AO as unknown as { __instances: MockOut[] }).__instances

const hasNonZero = (out: MockOut): boolean => out.writes.some((w) => w.some((s) => s !== 0))

describe('renderRelayClick', () => {
  test('produces a non-empty, normalised waveform per edge', () => {
    const on = renderRelayClick('on')
    const off = renderRelayClick('off')
    expect(on.length).toBeGreaterThan(500)
    expect(off.length).toBeGreaterThan(400)
    // peak normalised to ~0.85
    const peak = Math.max(...Array.from(on, Math.abs))
    expect(peak).toBeGreaterThan(0.8)
    expect(peak).toBeLessThanOrEqual(0.86)
    // the two edges differ (tick vs tock)
    expect(on.length).not.toBe(off.length)
  })

  test('is deterministic', () => {
    expect(Array.from(renderRelayClick('on'))).toEqual(Array.from(renderRelayClick('on')))
  })
})

describe('SystemSound', () => {
  beforeEach(() => {
    instances.length = 0
    vi.useFakeTimers()
    // Anchor the fake clock to an exact 500ms boundary → first edge at +500ms. Modern fake
    // timers drive Date.now(), so advancing timers advances the wall clock the generator reads.
    vi.setSystemTime(100_000)
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  const advance = (ms: number): void => {
    vi.advanceTimersByTime(ms)
  }

  test('starts an output on activation and emits a click after the first 500ms edge', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)

    expect(instances).toHaveLength(1)
    expect(instances[0]!.started).toBe(true)

    // before the first edge: silence only
    advance(300)
    expect(hasNonZero(instances[0]!)).toBe(false)

    // cross the 500ms boundary: a click appears
    advance(400)
    expect(hasNonZero(instances[0]!)).toBe(true)

    sound.dispose()
  })

  test('respects systemSoundsVolume = 0 (silent)', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0 }))
    sound.setBlinkerActive(true)
    advance(1200)
    expect(hasNonZero(instances[0]!)).toBe(false)
    sound.dispose()
  })

  test('does not start when audio output is disabled', () => {
    const sound = new SystemSound(() => ({ disableAudioOutput: true, systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)
    expect(instances).toHaveLength(0)
    sound.dispose()
  })

  test('passes the configured output device to the AudioOutput', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8, audioOutputDevice: 'spk-1' }))
    sound.setBlinkerActive(true)
    expect(instances[0]!.opts.device).toBe('spk-1')
    sound.dispose()
  })

  test('tears the output down after the grace period on deactivation', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)
    advance(200)
    sound.setBlinkerActive(false)
    expect(instances[0]!.stopped).toBe(false) // kept warm during grace
    advance(700) // past the 600ms grace
    expect(instances[0]!.stopped).toBe(true)
  })

  test('re-activation during the teardown grace keeps the same output alive', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)
    sound.setBlinkerActive(false)
    sound.setBlinkerActive(true)
    advance(700)
    expect(instances).toHaveLength(1)
    expect(instances[0]!.stopped).toBe(false)
    sound.dispose()
  })

  test('deactivation without prior activation is a no-op', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(false)
    expect(instances).toHaveLength(0)
  })

  test('deactivation while marked active without an output schedules no teardown', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 })) as any
    sound.active = true
    sound.setBlinkerActive(false)
    expect(sound.teardownTimer).toBeNull()
  })

  test('repeated deactivation replaces a pending teardown timer', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)
    sound.setBlinkerActive(false)
    ;(sound as any).active = true
    sound.setBlinkerActive(false)
    advance(700)
    expect(instances[0]!.stopped).toBe(true)
  })

  test('onDeviceChanged without an output is a no-op', () => {
    const sound = new SystemSound(() => ({}))
    sound.onDeviceChanged()
    expect(instances).toHaveLength(0)
  })

  test('onDeviceChanged reopens the output while active', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)
    sound.onDeviceChanged()
    expect(instances).toHaveLength(2)
    expect(instances[0]!.stopped).toBe(true)
    expect(instances[1]!.started).toBe(true)
    sound.dispose()
  })

  test('onDeviceChanged during the teardown grace closes without reopening', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 }))
    sound.setBlinkerActive(true)
    sound.setBlinkerActive(false)
    sound.onDeviceChanged()
    expect(instances).toHaveLength(1)
    expect(instances[0]!.stopped).toBe(true)
  })

  test('generate is a no-op without an output', () => {
    const sound = new SystemSound(() => ({})) as any
    expect(() => sound.generate()).not.toThrow()
  })

  test('generate produces nothing when no time has passed', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 })) as any
    sound.setBlinkerActive(true)
    advance(30)
    const writes = instances[0]!.writes.length
    sound.generate()
    expect(instances[0]!.writes.length).toBe(writes)
    sound.dispose()
  })

  test('caps catch-up after a long timer stall', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 0.8 })) as any
    sound.setBlinkerActive(true)
    sound.streamStartMs -= 10_000
    advance(30)
    expect(instances[0]!.writes[0]!.length).toBe(24000)
    sound.dispose()
  })

  test('falls back to the default volume when unset', () => {
    const sound = new SystemSound(() => ({}))
    sound.setBlinkerActive(true)
    advance(700)
    expect(hasNonZero(instances[0]!)).toBe(true)
    sound.dispose()
  })

  test('treats a non-finite volume as silence', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: Number.NaN }))
    sound.setBlinkerActive(true)
    advance(1200)
    expect(hasNonZero(instances[0]!)).toBe(false)
    sound.dispose()
  })

  test('clamps overdriven samples into the int16 range', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 1 })) as any
    sound.setBlinkerActive(true)
    sound.click = { wave: Float32Array.from([4, -4]), pos: 0 }
    advance(30)
    const w = instances[0]!.writes[0]!
    expect(w[0]).toBe(32767)
    expect(w[1]).toBe(32767)
    expect(w[2]).toBe(-32768)
    expect(w[3]).toBe(-32768)
    sound.dispose()
  })

  test('treats reads past the click wave as silence', () => {
    const sound = new SystemSound(() => ({ systemSoundsVolume: 1 })) as any
    sound.setBlinkerActive(true)
    sound.click = { wave: new Float32Array(0), pos: 0 }
    advance(30)
    expect(hasNonZero(instances[0]!)).toBe(false)
    expect(sound.click).toBeNull()
    sound.dispose()
  })
})
