import { ProjectionAudio } from '@main/services/projection/services/ProjectionAudio'

jest.mock('@main/services/audio', () => ({
  Microphone: jest.fn().mockImplementation(() => ({
    on: jest.fn(),
    start: jest.fn(),
    stop: jest.fn(),
    isCapturing: jest.fn(() => false)
  })),
  AudioOutput: jest.fn().mockImplementation(() => ({
    start: jest.fn(),
    stop: jest.fn(),
    write: jest.fn()
  })),
  downsampleToMono: jest.fn(() => new Int16Array([1, 2, 3]))
}))

jest.mock('@main/constants', () => ({
  DEBUG: false
}))

jest.mock('../../messages', () => ({
  decodeTypeMap: {
    1: { frequency: 48000, channel: 2, format: 'pcm', mimeType: 'audio/pcm', bitDepth: 16 },
    2: { frequency: 16000, channel: 1, format: 'pcm', mimeType: 'audio/pcm', bitDepth: 16 }
  },
  AudioData: class {}
}))

jest.mock('@shared/types/ProjectionEnums', () => ({
  AudioCommand: {
    AudioAttentionStart: 1,
    AudioAttentionRinging: 2,
    AudioPhonecallStop: 3,
    AudioSiriStart: 4,
    AudioSiriStop: 5,
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

function createSubject(config: Record<string, unknown> = { mediaDelay: 120 }) {
  return new ProjectionAudio(() => config as any, jest.fn(), jest.fn(), jest.fn()) as any
}

describe('ProjectionAudio state controls', () => {
  test('setInitialVolumes applies provided values and preserves defaults for omitted streams', () => {
    const a = createSubject()

    a.setInitialVolumes({ music: 0.3, nav: 0.4 })

    expect(a.volumes).toEqual({
      music: 0.3,
      nav: 0.4,
      siri: 1,
      call: 1
    })
  })

  test('setStreamVolume clamps values and ignores tiny no-op changes', () => {
    const a = createSubject()

    a.setStreamVolume('music', 2)
    expect(a.volumes.music).toBe(1)

    a.setStreamVolume('music', -5)
    expect(a.volumes.music).toBe(0)

    a.volumes.music = 0.5
    a.setStreamVolume('music', 0.50000001)
    expect(a.volumes.music).toBe(0.5)
  })

  test('setVisualizerEnabled toggles visualizer flag', () => {
    const a = createSubject()

    a.setVisualizerEnabled(true)
    expect(a.visualizerEnabled).toBe(true)

    a.setVisualizerEnabled(false)
    expect(a.visualizerEnabled).toBe(false)
  })

  test('resetForSessionStart clears stream/session state', () => {
    const a = createSubject()

    a.audioPlayers.set('k', { stop: jest.fn() })
    a.siriActive = true
    a.phonecallActive = true
    a.navActive = true
    a.mediaActive = true
    a.audioOpenArmed = true
    a.musicRampActive = true
    a.nextMusicRampStartAt = 123
    a.lastMusicDataAt = 123
    a.navMixQueue = [new Int16Array([1])]
    a.lastMusicPlayerKey = '1'
    a.lastNavPlayerKey = '2'
    a.uiCallIncoming = true

    a.resetForSessionStart()

    expect(a.siriActive).toBe(false)
    expect(a.phonecallActive).toBe(false)
    expect(a.navActive).toBe(false)
    expect(a.mediaActive).toBe(false)
    expect(a.audioOpenArmed).toBe(false)
    expect(a.musicRampActive).toBe(false)
    expect(a.nextMusicRampStartAt).toBe(0)
    expect(a.lastMusicDataAt).toBe(0)
    expect(a.navMixQueue).toEqual([])
    expect(a.lastMusicPlayerKey).toBeNull()
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.uiCallIncoming).toBe(false)
    expect(a.audioPlayers.size).toBe(0)
  })

  test('resetForSessionStop clears stream/session state', () => {
    const a = createSubject()

    a.audioPlayers.set('k', { stop: jest.fn() })
    a.siriActive = true
    a.phonecallActive = true
    a.navActive = true
    a.mediaActive = true
    a.audioOpenArmed = true
    a.musicRampActive = true
    a.nextMusicRampStartAt = 123
    a.lastMusicDataAt = 123
    a.navMixQueue = [new Int16Array([1])]
    a.lastMusicPlayerKey = '1'
    a.lastNavPlayerKey = '2'
    a.uiCallIncoming = true

    a.resetForSessionStop()

    expect(a.siriActive).toBe(false)
    expect(a.phonecallActive).toBe(false)
    expect(a.navActive).toBe(false)
    expect(a.mediaActive).toBe(false)
    expect(a.audioOpenArmed).toBe(false)
    expect(a.musicRampActive).toBe(false)
    expect(a.nextMusicRampStartAt).toBe(0)
    expect(a.lastMusicDataAt).toBe(0)
    expect(a.navMixQueue).toEqual([])
    expect(a.lastMusicPlayerKey).toBeNull()
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.uiCallIncoming).toBe(false)
    expect(a.audioPlayers.size).toBe(0)
  })

  test('gainFromVolume clamps invalid values and maps zero to zero', () => {
    const a = createSubject()

    expect(a.gainFromVolume(-1)).toBe(0)
    expect(a.gainFromVolume(Number.NaN)).toBe(0)
    expect(a.gainFromVolume(0)).toBe(0)
    expect(a.gainFromVolume(1)).toBeCloseTo(1, 5)
  })

  test('applyGain returns original pcm for unity or invalid gain', () => {
    const a = createSubject()
    const pcm = new Int16Array([100, -200])

    expect(a.applyGain(pcm, 1)).toBe(pcm)
    expect(a.applyGain(pcm, Number.NaN)).toBe(pcm)
  })

  test('applyGain returns silent buffer for zero or negative gain', () => {
    const a = createSubject()
    const pcm = new Int16Array([100, -200])

    expect(Array.from(a.applyGain(pcm, 0))).toEqual([0, 0])
    expect(Array.from(a.applyGain(pcm, -1))).toEqual([0, 0])
  })

  test('applyGain scales and clamps pcm values', () => {
    const a = createSubject()
    const pcm = new Int16Array([20000, -20000, 1000])

    expect(Array.from(a.applyGain(pcm, 2))).toEqual([32767, -32768, 2000])
  })

  test('processMusicChunk applies plain music gain when nav mixing is disabled', () => {
    const a = createSubject()
    const pcm = new Int16Array([1000, -1000])

    expect(Array.from(a.processMusicChunk(pcm, 0.5, 0, false))).toEqual([500, -500])
  })

  test('processMusicChunk mixes queued nav audio and advances queue state', () => {
    const a = createSubject()
    a.navMixQueue = [new Int16Array([100, 200]), new Int16Array([300])]
    a.navMixOffset = 0

    const out = a.processMusicChunk(new Int16Array([1000, 1000, 1000]), 1, 1, true)

    expect(Array.from(out)).toEqual([1100, 1200, 1300])
    expect(a.navMixQueue).toEqual([])
    expect(a.navMixOffset).toBe(0)
  })

  test('clearNavMix empties queue and resets offset', () => {
    const a = createSubject()
    a.navMixQueue = [new Int16Array([1, 2])]
    a.navMixOffset = 1

    a.clearNavMix()

    expect(a.navMixQueue).toEqual([])
    expect(a.navMixOffset).toBe(0)
  })

  test('getMediaDelay returns configured non-negative delay', () => {
    const a = createSubject({ mediaDelay: 250 })
    expect(a.getMediaDelay()).toBe(250)
  })

  test('getMediaDelay falls back to zero for invalid values', () => {
    expect(createSubject({ mediaDelay: -1 }).getMediaDelay()).toBe(0)
    expect(createSubject({ mediaDelay: Number.NaN }).getMediaDelay()).toBe(0)
    expect(createSubject({}).getMediaDelay()).toBe(0)
  })

  test('getLogicalStreamKey prioritizes call over siri over nav over music', () => {
    const a = createSubject()

    expect(a.getLogicalStreamKey({})).toBe('music')

    a.navActive = true
    expect(a.getLogicalStreamKey({})).toBe('nav')

    a.siriActive = true
    expect(a.getLogicalStreamKey({})).toBe('siri')

    a.phonecallActive = true
    expect(a.getLogicalStreamKey({})).toBe('call')
  })

  test('getAudioOutputForStream returns null for unknown decode type', () => {
    const a = createSubject()

    const out = a.getAudioOutputForStream({ decodeType: 999 })

    expect(out).toBeNull()
  })

  test('getAudioOutputForStream creates and reuses players by sampleRate and channels', () => {
    const a = createSubject()

    const first = a.getAudioOutputForStream({ decodeType: 1 })
    const second = a.getAudioOutputForStream({ decodeType: 1 })
    const third = a.getAudioOutputForStream({ decodeType: 2 })

    expect(first).toBeTruthy()
    expect(second).toBe(first)
    expect(third).not.toBe(first)
    expect(a.audioPlayers.size).toBe(2)
  })

  test('handleAudioData ignores music pcm when media is inactive', () => {
    const a = createSubject()
    const player = { write: jest.fn() }
    a.getAudioOutputForStream = jest.fn(() => player)
    a.getLogicalStreamKey = jest.fn(() => 'music')
    a.mediaActive = false

    a.handleAudioData({
      data: new Int16Array([1, 2, 3]),
      decodeType: 1
    })

    expect(player.write).not.toHaveBeenCalled()
  })

  test('handleAudioData writes pcm for nav-only playback when media is inactive', () => {
    const a = createSubject()
    const player = { write: jest.fn() }
    a.getAudioOutputForStream = jest.fn(() => player)
    a.getLogicalStreamKey = jest.fn(() => 'nav')
    a.mediaActive = false
    a.navActive = false

    a.handleAudioData({
      data: new Int16Array([1, 2, 3]),
      decodeType: 1
    })

    expect(player.write).toHaveBeenCalled()
  })

  test('handleAudioData enqueues nav mix and returns when media is active', () => {
    const a = createSubject()
    const player = { write: jest.fn() }
    a.getAudioOutputForStream = jest.fn(() => player)
    a.getLogicalStreamKey = jest.fn(() => 'nav')
    a.mediaActive = true
    a.navActive = true

    const chunk = new Int16Array([1, 2, 3])
    a.handleAudioData({
      data: chunk,
      decodeType: 1
    })

    expect(a.navMixQueue).toHaveLength(1)
    expect(Array.from(a.navMixQueue[0])).toEqual([1, 2, 3])
    expect(player.write).not.toHaveBeenCalled()
  })

  test('handleAudioData sends audioInfo only once when metadata is present', () => {
    const sendProjectionEvent = jest.fn()
    const a = new ProjectionAudio(
      () => ({ mediaDelay: 120 }) as any,
      sendProjectionEvent,
      jest.fn(),
      jest.fn()
    ) as any

    const player = { write: jest.fn() }
    a.getAudioOutputForStream = jest.fn(() => player)
    a.getLogicalStreamKey = jest.fn(() => 'nav')
    a.mediaActive = false

    a.handleAudioData({
      data: new Int16Array([1, 2]),
      decodeType: 1
    })

    a.handleAudioData({
      data: new Int16Array([3, 4]),
      decodeType: 1
    })

    const audioInfoCalls = sendProjectionEvent.mock.calls.filter(
      ([arg]) => arg?.type === 'audioInfo'
    )
    expect(audioInfoCalls).toHaveLength(1)
  })

  test('handleAudioData AudioOutputStart arms media open and resets music ramp state', () => {
    const a = createSubject()

    a.mediaActive = false
    a.handleAudioData({ command: 10 })

    expect(a.audioOpenArmed).toBe(true)
    expect(a.mediaActive).toBe(false)
    expect(a.musicRampActive).toBe(false)
    expect(a.musicFade.current).toBe(0)
    expect(a.musicFade.target).toBe(1)
  })

  test('handleAudioData AudioMediaStart implicitly starts media when not armed', () => {
    const a = createSubject()

    const before = Date.now()
    a.handleAudioData({ command: 11 })

    expect(a.mediaActive).toBe(true)
    expect(a.audioOpenArmed).toBe(false)
    expect(a.musicGateMuted).toBe(true)
    expect(a.nextMusicRampStartAt).toBeGreaterThanOrEqual(before + 120 - 5)
  })

  test('handleAudioData AudioMediaStart consumes open arm and starts media', () => {
    const a = createSubject()
    a.audioOpenArmed = true

    a.handleAudioData({ command: 11 })

    expect(a.audioOpenArmed).toBe(false)
    expect(a.mediaActive).toBe(true)
    expect(a.musicGateMuted).toBe(true)
  })

  test('handleAudioData AudioMediaStop deactivates media and clears music player', () => {
    const a = createSubject()
    a.mediaActive = true
    a.audioOpenArmed = true
    a.lastMusicPlayerKey = 'music-key'
    a.stopPlayerByKey = jest.fn()

    a.handleAudioData({ command: 12 })

    expect(a.mediaActive).toBe(false)
    expect(a.audioOpenArmed).toBe(false)
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('music-key')
    expect(a.lastMusicPlayerKey).toBeNull()
  })

  test('handleAudioData nav start activates nav and prepares ducking', () => {
    const a = createSubject()
    a.mediaActive = true
    a.siriActive = false
    a.phonecallActive = false
    a.clearNavMix = jest.fn()

    a.handleAudioData({ command: 6 })

    expect(a.navActive).toBe(true)
    expect(a.navHoldUntil).toBe(0)
    expect(a.clearNavMix).toHaveBeenCalled()
    expect(a.musicRampActive).toBe(true)
    expect(a.musicFade.target).toBe(a.navDuckingTarget)
  })

  test('handleAudioData nav stop clears nav and removes nav-only player when media inactive', () => {
    const a = createSubject()
    a.mediaActive = false
    a.navActive = true
    a.lastNavPlayerKey = 'nav-key'
    a.clearNavMix = jest.fn()
    a.stopPlayerByKey = jest.fn()

    const before = Date.now()
    a.handleAudioData({ command: 8 })

    expect(a.navActive).toBe(false)
    expect(a.clearNavMix).toHaveBeenCalled()
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('nav-key')
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.navHoldUntil).toBeGreaterThanOrEqual(before)
  })

  test('handleAudioData AudioOutputStop stops remembered players when no call or siri is active', () => {
    const a = createSubject()
    a.lastMusicPlayerKey = 'music'
    a.lastNavPlayerKey = 'nav'
    a.lastSiriPlayerKey = 'siri'
    a.lastCallPlayerKey = 'call'
    a.stopPlayerByKey = jest.fn()

    a.handleAudioData({ command: 13 })

    expect(a.stopPlayerByKey).toHaveBeenCalledWith('music')
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('nav')
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('siri')
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('call')
    expect(a.lastMusicPlayerKey).toBeNull()
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.lastSiriPlayerKey).toBeNull()
    expect(a.lastCallPlayerKey).toBeNull()
  })

  test('handleAudioData AudioInputConfig updates current mic decode type', () => {
    const a = createSubject()

    a.handleAudioData({ command: 14, decodeType: 2 })

    expect(a.currentMicDecodeType).toBe(2)
  })

  test('handleAudioData AudioSiriStart updates siri state and skips mic start without decodeType', () => {
    const a = createSubject({ micType: 0, audioTransferMode: false })

    a.handleAudioData({ command: 4 })

    expect(a.siriActive).toBe(true)
    expect(a.phonecallActive).toBe(false)
    expect(a.currentMicDecodeType).toBeNull()
  })

  test('handleAudioData AudioPhonecallStart updates phone state and stops mic in transfer mode', () => {
    const a = createSubject({ micType: 1, audioTransferMode: true })
    a._mic = { stop: jest.fn() }

    a.handleAudioData({ command: 15, decodeType: 1 })

    expect(a.phonecallActive).toBe(true)
    expect(a.siriActive).toBe(false)
    expect(a._mic.stop).toHaveBeenCalled()
  })

  test('handleAudioData AudioSiriStop clears siri state and stops siri player/mic', () => {
    const a = createSubject()
    a.siriActive = true
    a.lastSiriPlayerKey = 'siri-key'
    a.stopPlayerByKey = jest.fn()
    a._mic = { stop: jest.fn() }

    a.handleAudioData({ command: 5 })

    expect(a.siriActive).toBe(false)
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('siri-key')
    expect(a.lastSiriPlayerKey).toBeNull()
    expect(a._mic.stop).toHaveBeenCalled()
  })

  test('handleAudioData AudioPhonecallStop clears phone state and stops mic', () => {
    const a = createSubject()
    a.phonecallActive = true
    a._mic = { stop: jest.fn() }

    a.handleAudioData({ command: 3 })

    expect(a.phonecallActive).toBe(false)
    expect(a._mic.stop).toHaveBeenCalled()
  })
})
