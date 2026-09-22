import { AudioCommand } from '@shared/types/ProjectionEnums'
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest'
import type { AudioData } from '../messages'

const micTap = vi.hoisted(() => {
  const taps: Array<{ close: ReturnType<typeof vi.fn> }> = []
  const open = vi.fn(() => {
    const tap = { close: vi.fn() }
    taps.push(tap)
    return tap
  })
  return { taps, open }
})

vi.mock('@main/services/audio/micTap', () => ({ MicTap: { open: micTap.open } }))

import { ScoAudio } from '../ScoAudio'

type Deps = ConstructorParameters<typeof ScoAudio>[0]

// Deps around a call stream registry, announce() plays the host opening the stream
function makeDeps(over: Partial<Deps> = {}) {
  const listeners = new Set<(streamId: number) => void>()
  const emitted: AudioData[] = []
  const mocks = {
    emitAudio: vi.fn((m: AudioData) => {
      emitted.push(m)
    }),
    getMicDevice: vi.fn((): string | undefined => 'mic0'),
    primeCall: vi.fn(),
    dropCall: vi.fn(),
    onCallStream: vi.fn((cb: (streamId: number) => void) => {
      listeners.add(cb)
      return () => {
        listeners.delete(cb)
      }
    }),
    feedPath: vi.fn(async () => '/tmp/feed.sock'),
    setScoSink: vi.fn(async (): Promise<unknown> => ({ ok: true }))
  }
  const deps: Deps = { ...mocks, ...over }
  const announce = (streamId: number) => {
    for (const cb of listeners) cb(streamId)
  }
  return { deps, mocks, emitted, announce, listeners }
}

// Lets the feed lookup and the sink call settle
const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

describe('ScoAudio', () => {
  let warn: ReturnType<typeof vi.spyOn>
  let log: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    log = vi.spyOn(console, 'log').mockImplementation(() => {})
  })

  afterEach(() => {
    warn.mockRestore()
    log.mockRestore()
    micTap.taps.length = 0
    vi.clearAllMocks()
  })

  test('start announces the call, primes the call stream and opens the tap', () => {
    const { deps, mocks, emitted } = makeDeps()
    const sco = new ScoAudio(deps)

    sco.start()

    expect(emitted).toHaveLength(1)
    expect(emitted[0].command).toBe(AudioCommand.AudioPhonecallStart)
    expect(emitted[0].audioType).toBe(2)
    expect(mocks.primeCall).toHaveBeenCalledTimes(1)
    expect(micTap.open).toHaveBeenCalledWith('/tmp/aa-sco.mic', {
      sampleRate: 8000,
      channels: 1,
      device: 'mic0'
    })
    expect(mocks.setScoSink).not.toHaveBeenCalled()
  })

  test('the call stream id reaches the helper together with the feed', async () => {
    const { deps, mocks, announce } = makeDeps()
    const sco = new ScoAudio(deps)
    sco.start()

    announce(17)
    await flush()

    expect(mocks.feedPath).toHaveBeenCalledTimes(1)
    expect(mocks.setScoSink).toHaveBeenCalledWith('/tmp/feed.sock', 17)
  })

  test('stop closes the tap, clears the sink, drops the call and announces the end', () => {
    const { deps, mocks, emitted, listeners } = makeDeps()
    const sco = new ScoAudio(deps)
    sco.start()
    emitted.length = 0

    sco.stop()

    expect(micTap.taps[0].close).toHaveBeenCalledTimes(1)
    expect(mocks.setScoSink).toHaveBeenCalledWith()
    expect(mocks.dropCall).toHaveBeenCalledTimes(1)
    expect(listeners.size).toBe(0)
    expect(emitted).toHaveLength(1)
    expect(emitted[0].command).toBe(AudioCommand.AudioPhonecallStop)
  })

  test('a stream id that arrives after stop goes nowhere', async () => {
    const { deps, mocks, announce } = makeDeps()
    const sco = new ScoAudio(deps)
    sco.start()
    sco.stop()

    announce(17)
    await flush()

    // Only the clear from stop, no sink for the late id
    expect(mocks.setScoSink).toHaveBeenCalledTimes(1)
    expect(mocks.setScoSink).toHaveBeenCalledWith()
  })

  test('a stop during the feed lookup leaves the sink alone', async () => {
    let resolveFeed: (path: string) => void = () => {}
    const feedPath = vi.fn(
      () =>
        new Promise<string>((resolve) => {
          resolveFeed = resolve
        })
    )
    const { deps, mocks, announce } = makeDeps({ feedPath })
    const sco = new ScoAudio(deps)
    sco.start()
    announce(17)
    sco.stop()

    resolveFeed('/tmp/feed.sock')
    await flush()

    expect(mocks.setScoSink).toHaveBeenCalledTimes(1)
    expect(mocks.setScoSink).toHaveBeenCalledWith()
  })

  test('without a feed the helper is not told and the caller stays silent', async () => {
    const { deps, mocks, announce } = makeDeps({ feedPath: vi.fn(async () => '') })
    const sco = new ScoAudio(deps)
    sco.start()

    announce(5)
    await flush()

    expect(mocks.setScoSink).not.toHaveBeenCalled()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no media feed'))
  })

  test('a helper that refuses the sink is logged, the call and its end go on', async () => {
    const setScoSink = vi.fn(async () => {
      throw new Error('busy')
    })
    const { deps, mocks, announce } = makeDeps({ setScoSink })
    const sco = new ScoAudio(deps)
    sco.start()

    announce(5)
    await flush()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('busy'))

    expect(() => sco.stop()).not.toThrow()
    await flush()
    expect(mocks.dropCall).toHaveBeenCalledTimes(1)
  })

  test('a missing tap is logged, the call itself goes on', () => {
    micTap.open.mockReturnValueOnce(null as never)
    const { deps, mocks, emitted } = makeDeps()
    const sco = new ScoAudio(deps)

    sco.start()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('no microphone tap'))
    expect(mocks.primeCall).toHaveBeenCalledTimes(1)

    sco.stop()
    expect(mocks.dropCall).toHaveBeenCalledTimes(1)
    expect(emitted.at(-1)?.command).toBe(AudioCommand.AudioPhonecallStop)
  })

  test('start is idempotent', () => {
    const { deps, mocks, emitted } = makeDeps()
    const sco = new ScoAudio(deps)

    sco.start()
    sco.start()

    expect(micTap.open).toHaveBeenCalledTimes(1)
    expect(mocks.primeCall).toHaveBeenCalledTimes(1)
    expect(emitted).toHaveLength(1)
  })

  test('stop before start is a no-op', () => {
    const { deps, mocks, emitted } = makeDeps()
    const sco = new ScoAudio(deps)

    sco.stop()

    expect(emitted).toHaveLength(0)
    expect(mocks.setScoSink).not.toHaveBeenCalled()
    expect(mocks.dropCall).not.toHaveBeenCalled()
  })
})
