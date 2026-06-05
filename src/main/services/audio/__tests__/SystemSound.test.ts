jest.mock('../AudioOutput', () => {
  const instances: Array<{
    started: boolean
    stopped: boolean
    writes: Int16Array[]
    opts: unknown
  }> = []
  return {
    __instances: instances,
    AudioOutput: class {
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

import * as AO from '../AudioOutput'
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
    jest.useFakeTimers()
    // Anchor the fake clock to an exact 500ms boundary → first edge at +500ms. Modern fake
    // timers drive Date.now(), so advancing timers advances the wall clock the generator reads.
    jest.setSystemTime(100_000)
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  const advance = (ms: number): void => {
    jest.advanceTimersByTime(ms)
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
})
