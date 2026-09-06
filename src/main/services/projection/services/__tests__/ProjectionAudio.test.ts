import { HostAudioOutput } from '@main/services/audio'
import { ProjectionAudio } from '@main/services/projection/services/ProjectionAudio'

const gstHostMock = vi.hoisted(() => {
  let vizCb: ((s: Uint8Array, rate: number) => void) | null = null
  return {
    setVisualizerTap: vi.fn(),
    setAudioVolume: vi.fn(),
    onVisualizerAudio: (cb: (s: Uint8Array, rate: number) => void) => {
      vizCb = cb
    },
    emitViz: (s: Uint8Array, rate = 48000) => vizCb?.(s, rate)
  }
})

vi.mock('@main/services/video/gstHost', () => ({ gstHost: gstHostMock }))

vi.mock('@main/services/video/GstVideo', () => ({
  useHostProcess: true,
  onAudioReceiverVisualizer: vi.fn(),
  setAudioReceiverVisualizerTap: vi.fn()
}))

vi.mock('@main/services/audio', () => ({
  HostAudioOutput: vi.fn().mockImplementation(function () {
    return {
      start: vi.fn(),
      stop: vi.fn(),
      setVolume: vi.fn()
    }
  })
}))

vi.mock('@main/constants', () => ({
  DEBUG: false
}))

vi.mock('@shared/types/ProjectionEnums', () => ({
  AudioCommand: {
    AudioAttentionStart: 1,
    AudioAttentionRinging: 2,
    AudioPhonecallStop: 3,
    AudioVoiceAssistantStart: 4,
    AudioVoiceAssistantStop: 5,
    AudioNaviStart: 6,
    AudioTurnByTurnStart: 7,
    AudioNaviStop: 8,
    AudioTurnByTurnStop: 9,
    AudioOutputStart: 10,
    AudioMediaStart: 11,
    AudioMediaStop: 12,
    AudioOutputStop: 13,
    AudioInputConfig: 14,
    AudioPhonecallStart: 15
  }
}))

type Subject = ProjectionAudio & Record<string, any>

function createSubject(
  config: Record<string, unknown> = { mediaDelay: 120 },
  applyStreamVolume = vi.fn(),
  sendProjectionEvent = vi.fn()
): Subject {
  return new ProjectionAudio(
    () => config as never,
    sendProjectionEvent,
    vi.fn(),
    applyStreamVolume
  ) as Subject
}

// A HostAudioOutput stand-in that opens with a fixed stream id on start().
const opened: Array<{
  setVolume: ReturnType<typeof vi.fn>
  stop: ReturnType<typeof vi.fn>
  device?: string
}> = []

function openingOutput(streamId: number) {
  return function (opts: { onOpened?: (id: number) => void; device?: string }) {
    const o = {
      device: opts.device,
      hostStreamId: null as number | null,
      start: vi.fn(() => {
        o.hostStreamId = streamId
        opts.onOpened?.(streamId)
      }),
      stop: vi.fn(),
      setVolume: vi.fn()
    }
    opened.push(o)
    return o
  }
}

beforeEach(() => {
  opened.length = 0
  vi.mocked(HostAudioOutput).mockClear()
  gstHostMock.setVisualizerTap.mockClear()
})

describe('ProjectionAudio levels', () => {
  test('setInitialVolumes applies provided values, keeps the rest and pushes every level', () => {
    const apply = vi.fn()
    const a = createSubject(undefined, apply)
    a.setInitialVolumes({ music: 0.3, nav: 0.4 })
    expect(a.volumes).toEqual({ music: 0.3, nav: 0.4, voiceAssistant: 1, call: 1 })
    expect(apply).toHaveBeenCalledWith(3, 0.3, 0)
    expect(apply).toHaveBeenCalledWith(4, 0.4, 0)
    expect(apply).toHaveBeenCalledWith(1, 1, 0)
    expect(apply).toHaveBeenCalledWith(2, 1, 0)
  })

  test('setStreamVolume clamps, ignores tiny changes and ramps down faster than up', () => {
    const apply = vi.fn()
    const a = createSubject(undefined, apply)
    a.setStreamVolume('music', 2)
    expect(a.volumes.music).toBe(1)
    apply.mockClear()
    a.setStreamVolume('music', 0.5)
    expect(apply).toHaveBeenLastCalledWith(3, 0.5, 500)
    a.setStreamVolume('music', 0.50000001)
    expect(apply).toHaveBeenCalledTimes(1)
    a.setStreamVolume('music', 0.8)
    expect(apply).toHaveBeenLastCalledWith(3, 0.8, 1500)
    a.setStreamVolume('nav', Number.NaN)
    expect(a.volumes.nav).toBe(0)
    a.setStreamVolume('' as never, 0.5)
    expect(apply).toHaveBeenCalledTimes(3)
  })

  test('without an applyStreamVolume hook the default no-op runs', () => {
    const a = new ProjectionAudio(() => ({}) as never, vi.fn(), vi.fn())
    expect(() => a.setStreamVolume('music', 0.5)).not.toThrow()
  })

  test('setInitialVolumes takes every provided stream and keeps every omitted one', () => {
    const a = createSubject()
    a.setInitialVolumes({ voiceAssistant: 0.3, call: 0.4 })
    expect(a.volumes).toEqual({ music: 1, nav: 1, voiceAssistant: 0.3, call: 0.4 })
    a.setInitialVolumes({})
    expect(a.volumes).toEqual({ music: 1, nav: 1, voiceAssistant: 0.3, call: 0.4 })
  })

  test('duck, unduck and restoreDuck scale the music level with their ramp', () => {
    const apply = vi.fn()
    const a = createSubject(undefined, apply)
    a.setInitialVolumes({ music: 0.8 })
    apply.mockClear()
    a.duck(0.25, 300)
    expect(apply).toHaveBeenLastCalledWith(3, 0.2, 300)
    a.unduck(-5)
    expect(apply).toHaveBeenLastCalledWith(3, 0.8, 0)
    a.restoreDuck(1.5, 50)
    expect(a.duckLevel).toBe(1)
    expect(apply).toHaveBeenLastCalledWith(3, 0.8, 50)
  })
})

describe('ProjectionAudio host streams', () => {
  test('a primed stream opens at the current level and follows setHostStreamVolume', () => {
    vi.mocked(HostAudioOutput).mockImplementationOnce(openingOutput(42) as never)
    const a = createSubject()
    a.setStreamVolume('music', 0.5)
    a.primeOutput(3, 48000, 2)
    expect(opened[0].setVolume).toHaveBeenCalledWith(0.5, 0)
    a.setHostStreamVolume(3, 0.25, 80)
    expect(opened[0].setVolume).toHaveBeenLastCalledWith(0.25, 80)
    a.setHostStreamVolume(4, 0.1, 0)
    expect(opened[0].setVolume).toHaveBeenCalledTimes(2)
  })

  test('primeOutput keeps channels apart, same type and format open separate streams', () => {
    vi.mocked(HostAudioOutput)
      .mockImplementationOnce(openingOutput(21) as never)
      .mockImplementationOnce(openingOutput(22) as never)
    const a = createSubject()
    const seen: Array<[number, number, string | undefined]> = []
    const off = a.onHostOutput((audioType, streamId, tag) => {
      seen.push([audioType, streamId, tag])
    })
    a.primeOutput(4, 16000, 1, 'speech')
    a.primeOutput(4, 16000, 1, 'system')
    a.primeOutput(4, 16000, 1, 'speech')
    a.primeOutput(4, 0, 1, 'speech')
    expect(seen).toEqual([
      [4, 21, 'speech'],
      [4, 22, 'system']
    ])
    expect(a.hostOutputs()).toEqual([
      { audioType: 4, streamId: 21, tag: 'speech' },
      { audioType: 4, streamId: 22, tag: 'system' }
    ])
    off()
    a.primeOutput(3, 48000, 2)
    expect(seen).toHaveLength(2)
  })

  test('a device change reopens the streams on the new device and announces them', () => {
    vi.mocked(HostAudioOutput)
      .mockImplementationOnce(openingOutput(31) as never)
      .mockImplementationOnce(openingOutput(32) as never)
    const config: Record<string, unknown> = { mediaDelay: 120, audioOutputDevice: 'bt-headset' }
    const a = createSubject(config)
    const seen: Array<[number, number, string | undefined]> = []
    a.onHostOutput((audioType, streamId, tag) => {
      seen.push([audioType, streamId, tag])
    })
    a.primeOutput(3, 48000, 2, 'media')
    config.audioOutputDevice = 'speakers'
    a.onAudioDeviceChanged()
    expect(opened[0].stop).toHaveBeenCalled()
    expect(seen).toEqual([
      [3, 31, 'media'],
      [3, 32, 'media']
    ])
    expect(opened.map((o) => o.device)).toEqual(['bt-headset', 'speakers'])
    expect(a.hostOutputs()).toEqual([{ audioType: 3, streamId: 32, tag: 'media' }])
  })

  test('dropPrimed stops the streams of one tag and leaves the others', () => {
    vi.mocked(HostAudioOutput)
      .mockImplementationOnce(openingOutput(51) as never)
      .mockImplementationOnce(openingOutput(52) as never)
    const a = createSubject()
    a.primeOutput(2, 8000, 1, 'call')
    a.primeOutput(3, 48000, 2, 'media')
    a.dropPrimed('call')
    expect(opened[0].stop).toHaveBeenCalled()
    expect(opened[1].stop).not.toHaveBeenCalled()
    expect(a.hostOutputs()).toEqual([{ audioType: 3, streamId: 52, tag: 'media' }])
    expect(() => a.dropPrimed('nothing')).not.toThrow()
  })

  test('a dropped stream stays closed when a device change reopens the primed ones', () => {
    vi.mocked(HostAudioOutput)
      .mockImplementationOnce(openingOutput(61) as never)
      .mockImplementationOnce(openingOutput(62) as never)
      .mockImplementationOnce(openingOutput(63) as never)
    const a = createSubject()
    a.primeOutput(2, 8000, 1, 'call')
    a.primeOutput(3, 48000, 2, 'media')
    a.dropPrimed('call')
    a.onAudioDeviceChanged()
    expect(opened).toHaveLength(3)
    expect(a.hostOutputs()).toEqual([{ audioType: 3, streamId: 63, tag: 'media' }])
  })

  test('a session reset stops every stream, a failing stop is ignored', () => {
    vi.mocked(HostAudioOutput)
      .mockImplementationOnce(openingOutput(71) as never)
      .mockImplementationOnce(function () {
        return {
          hostStreamId: 72,
          start: vi.fn(),
          stop: vi.fn(() => {
            throw new Error('boom')
          }),
          setVolume: vi.fn()
        }
      } as never)
    const a = createSubject()
    a.primeOutput(3, 48000, 2, 'media')
    a.primeOutput(4, 16000, 1, 'speech')
    a.duck(0.2, 100)
    a.uiNavHintActive = true
    expect(() => a.resetForSessionStop()).not.toThrow()
    expect(opened[0].stop).toHaveBeenCalled()
    expect(a.hostOutputs()).toEqual([])
    expect(a.duckLevel).toBe(1)
    expect(a.uiNavHintActive).toBe(false)
    a.resetForSessionStart()
    expect(a.hostOutputs()).toEqual([])
  })

  test('stopPlayerByKey ignores a null or unknown key', () => {
    const a = createSubject()
    expect(() => a.stopPlayerByKey(null)).not.toThrow()
    expect(() => a.stopPlayerByKey('nope')).not.toThrow()
  })

  test('a stream for an unmapped audio type opens at unity gain', () => {
    vi.mocked(HostAudioOutput).mockImplementationOnce(openingOutput(88) as never)
    const a = createSubject()
    a.primeOutput(99, 16000, 1, 'other')
    expect(opened[0].setVolume).toHaveBeenCalledWith(1, 0)
    a.setHostStreamVolume(99, 0.3, 0)
    expect(opened[0].setVolume).toHaveBeenLastCalledWith(0.3, 0)
  })

  test('a stream that is still opening is left out of hostOutputs', () => {
    const a = createSubject()
    a.primeOutput(3, 48000, 2, 'media')
    expect(a.hostOutputs()).toEqual([])
  })

  test('setHostStreamVolume leaves a stream that is still opening alone', () => {
    const a = createSubject()
    a.primeOutput(3, 48000, 2, 'media')
    expect(() => a.setHostStreamVolume(3, 0.5, 0)).not.toThrow()
  })
})

describe('ProjectionAudio visualizer', () => {
  test('host viz samples reach the renderer while a window wants them', () => {
    const sendChunked = vi.fn()
    const a = new ProjectionAudio(() => ({}) as never, vi.fn(), sendChunked, vi.fn())
    gstHostMock.emitViz(new Uint8Array([1, 2]))
    expect(sendChunked).not.toHaveBeenCalled()
    a.setVisualizerEnabled(true)
    expect(gstHostMock.setVisualizerTap).toHaveBeenCalledWith(true)
    gstHostMock.emitViz(new Uint8Array([1, 2, 3, 4]), 44100)
    expect(sendChunked).toHaveBeenCalledWith(
      'projection-audio-chunk',
      expect.anything(),
      64 * 1024,
      { sampleRate: 44100, channels: 1 }
    )
    gstHostMock.emitViz(new Uint8Array([]), 44100)
    expect(sendChunked).toHaveBeenCalledTimes(1)
    a.setVisualizerEnabled(false)
    expect(gstHostMock.setVisualizerTap).toHaveBeenLastCalledWith(false)
  })

  test('the tap is reference counted per window', () => {
    const a = createSubject()
    a.setVisualizerEnabled(true, 1)
    a.setVisualizerEnabled(true, 2)
    expect(a.visualizerEnabled).toBe(true)
    expect(gstHostMock.setVisualizerTap).toHaveBeenCalledTimes(1)
    a.setVisualizerEnabled(false, 1)
    expect(a.visualizerEnabled).toBe(true)
    a.setVisualizerEnabled(false, 2)
    expect(a.visualizerEnabled).toBe(false)
    expect(gstHostMock.setVisualizerTap).toHaveBeenLastCalledWith(false)
  })
})

describe('ProjectionAudio attention hints', () => {
  const command = (cmd: number, extra: Record<string, unknown> = {}) =>
    ({ decodeType: 1, audioType: 3, command: cmd, ...extra }) as never

  test('an incoming call is announced once until the call ends', () => {
    const events = vi.fn()
    const a = createSubject(undefined, vi.fn(), events)
    a.handleAudioData(command(1))
    a.handleAudioData(command(2))
    expect(events).toHaveBeenCalledTimes(1)
    expect(events).toHaveBeenCalledWith({
      type: 'attention',
      payload: { kind: 'call', active: true, phase: 'incoming' }
    })
    a.handleAudioData(command(3))
    expect(events).toHaveBeenLastCalledWith({
      type: 'attention',
      payload: { kind: 'call', active: false, phase: 'ended' }
    })
    a.handleAudioData(command(3))
    expect(events).toHaveBeenCalledTimes(2)
  })

  test('the voice assistant hint is raised once', () => {
    const events = vi.fn()
    const a = createSubject(undefined, vi.fn(), events)
    a.handleAudioData(command(4))
    a.handleAudioData(command(4))
    expect(events).toHaveBeenCalledTimes(1)
    expect(events).toHaveBeenCalledWith({
      type: 'attention',
      payload: { kind: 'voiceAssistant', active: true }
    })
  })

  test('the nav hint follows navigation and turn by turn start and stop', () => {
    const events = vi.fn()
    const a = createSubject(undefined, vi.fn(), events)
    a.handleAudioData(command(8))
    expect(events).not.toHaveBeenCalled()
    a.handleAudioData(command(6))
    a.handleAudioData(command(7))
    expect(events).toHaveBeenCalledTimes(1)
    expect(events).toHaveBeenCalledWith({
      type: 'attention',
      payload: { kind: 'nav', active: true }
    })
    a.handleAudioData(command(9))
    expect(events).toHaveBeenLastCalledWith({
      type: 'attention',
      payload: { kind: 'nav', active: false }
    })
  })

  test('messages without a command and stream commands change nothing', () => {
    const events = vi.fn()
    const a = createSubject(undefined, vi.fn(), events)
    a.handleAudioData({ decodeType: 1, audioType: 3 } as never)
    for (const cmd of [10, 11, 12, 13, 14, 15, 5]) a.handleAudioData(command(cmd))
    expect(events).not.toHaveBeenCalled()
  })
})

describe('ProjectionAudio non-host-process visualizer path', () => {
  test('forwards the receiver-side visualizer callback and tap toggle when not using the host process', async () => {
    vi.resetModules()
    let capturedCb: ((s: Uint8Array, rate: number) => void) | null = null
    const setAudioReceiverVisualizerTap = vi.fn()
    vi.doMock('@main/services/video/GstVideo', () => ({
      useHostProcess: false,
      onAudioReceiverVisualizer: (cb: (s: Uint8Array, rate: number) => void) => {
        capturedCb = cb
      },
      setAudioReceiverVisualizerTap
    }))
    const { ProjectionAudio: NonHostProjectionAudio } = await import(
      '@main/services/projection/services/ProjectionAudio'
    )
    const sendChunked = vi.fn()
    const a = new NonHostProjectionAudio(() => ({}) as never, vi.fn(), sendChunked, vi.fn())

    // useHostProcess === false takes the receiver-tap else branch instead of gstHost's.
    a.setVisualizerEnabled(true)
    expect(setAudioReceiverVisualizerTap).toHaveBeenCalledWith(true)

    expect(capturedCb).toBeTypeOf('function')
    capturedCb?.(new Uint8Array([1, 2, 3, 4]), 44100)
    expect(sendChunked).toHaveBeenCalledWith(
      'projection-audio-chunk',
      expect.anything(),
      64 * 1024,
      { sampleRate: 44100, channels: 1 }
    )

    vi.doUnmock('@main/services/video/GstVideo')
  })
})

describe('ProjectionAudio DEBUG logging', () => {
  test('logs commands and device changes when DEBUG is on', async () => {
    vi.resetModules()
    vi.doMock('@main/constants', () => ({ DEBUG: true }))
    const { ProjectionAudio: Debugging } = await import(
      '@main/services/projection/services/ProjectionAudio'
    )
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => {})
    const a = new Debugging(() => ({}) as never, vi.fn(), vi.fn(), vi.fn())
    a.handleAudioData({ decodeType: 1, audioType: 3, command: 6 } as never)
    a.onAudioDeviceChanged()
    expect(debug).toHaveBeenCalledTimes(2)
    debug.mockRestore()
    vi.doUnmock('@main/constants')
  })
})
