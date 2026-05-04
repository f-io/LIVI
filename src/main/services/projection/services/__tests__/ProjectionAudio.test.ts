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
      voiceAssistant: 1,
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
    a.voiceAssistantActive = true
    a.phonecallActive = true
    a.navActive = true
    a.mediaActive = true
    a.audioOpenArmed = true
    a.musicRampActive = true
    a.nextMusicRampStartAt = 123
    a.lastMusicDataAt = 123
    a.lastMusicPlayerKey = '1'
    a.lastNavPlayerKey = '2'
    a.uiCallIncoming = true

    a.resetForSessionStart()

    expect(a.voiceAssistantActive).toBe(false)
    expect(a.phonecallActive).toBe(false)
    expect(a.navActive).toBe(false)
    expect(a.mediaActive).toBe(false)
    expect(a.audioOpenArmed).toBe(false)
    expect(a.musicRampActive).toBe(false)
    expect(a.nextMusicRampStartAt).toBe(0)
    expect(a.lastMusicDataAt).toBe(0)
    expect(a.lastMusicPlayerKey).toBeNull()
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.uiCallIncoming).toBe(false)
    expect(a.audioPlayers.size).toBe(0)
  })

  test('resetForSessionStop clears stream/session state', () => {
    const a = createSubject()

    a.audioPlayers.set('k', { stop: jest.fn() })
    a.voiceAssistantActive = true
    a.phonecallActive = true
    a.navActive = true
    a.mediaActive = true
    a.audioOpenArmed = true
    a.musicRampActive = true
    a.nextMusicRampStartAt = 123
    a.lastMusicDataAt = 123
    a.lastMusicPlayerKey = '1'
    a.lastNavPlayerKey = '2'
    a.uiCallIncoming = true

    a.resetForSessionStop()

    expect(a.voiceAssistantActive).toBe(false)
    expect(a.phonecallActive).toBe(false)
    expect(a.navActive).toBe(false)
    expect(a.mediaActive).toBe(false)
    expect(a.audioOpenArmed).toBe(false)
    expect(a.musicRampActive).toBe(false)
    expect(a.nextMusicRampStartAt).toBe(0)
    expect(a.lastMusicDataAt).toBe(0)
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

  test('getMediaDelay returns configured non-negative delay', () => {
    const a = createSubject({ mediaDelay: 250 })
    expect(a.getMediaDelay()).toBe(250)
  })

  test('getMediaDelay falls back to zero for invalid values', () => {
    expect(createSubject({ mediaDelay: -1 }).getMediaDelay()).toBe(0)
    expect(createSubject({ mediaDelay: Number.NaN }).getMediaDelay()).toBe(0)
    expect(createSubject({}).getMediaDelay()).toBe(0)
  })

  test('getLogicalStreamKey prioritizes call over voiceAssistant over nav over music', () => {
    const a = createSubject()

    expect(a.getLogicalStreamKey({})).toBe('music')

    a.navActive = true
    expect(a.getLogicalStreamKey({})).toBe('nav')

    a.voiceAssistantActive = true
    expect(a.getLogicalStreamKey({})).toBe('voiceAssistant')

    a.phonecallActive = true
    expect(a.getLogicalStreamKey({})).toBe('call')
  })

  test('getAudioOutputForStream returns null for unknown decode type', () => {
    const a = createSubject()

    const out = a.getAudioOutputForStream('music', 1, { decodeType: 999 })

    expect(out).toBeNull()
  })

  test('getAudioOutputForStream creates and reuses players by (logicalKey, audioType, rate, channels)', () => {
    const a = createSubject()

    const musicA = a.getAudioOutputForStream('music', 1, { decodeType: 1 })
    const musicB = a.getAudioOutputForStream('music', 1, { decodeType: 1 })
    const musicC = a.getAudioOutputForStream('music', 1, { decodeType: 2 })
    // Same wire format but different audioType → separate sink-input.
    const navSameFormat = a.getAudioOutputForStream('nav', 2, { decodeType: 1 })

    expect(musicA).toBeTruthy()
    expect(musicB).toBe(musicA)
    expect(musicC).not.toBe(musicA)
    expect(navSameFormat).not.toBe(musicA)
    expect(a.audioPlayers.size).toBe(3)
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

  test('handleAudioData writes nav PCM to its own player even when media is active', () => {
    // The OS sink mixes the nav stream with the music stream natively, so we
    // just write to the nav player directly and let the OS handle the mix.
    const a = createSubject()
    const player = { write: jest.fn() }
    a.getAudioOutputForStream = jest.fn(() => player)
    a.getLogicalStreamKey = jest.fn(() => 'nav')
    a.mediaActive = true
    a.navActive = true

    a.handleAudioData({
      data: new Int16Array([1, 2, 3]),
      decodeType: 1
    })

    expect(player.write).toHaveBeenCalled()
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
    a.voiceAssistantActive = false
    a.phonecallActive = false

    a.handleAudioData({ command: 6 })

    expect(a.navActive).toBe(true)
    expect(a.navHoldUntil).toBe(0)
    expect(a.musicRampActive).toBe(true)
    expect(a.musicFade.target).toBe(a.navDuckingTarget)
  })

  test('handleAudioData nav stop clears nav and removes nav-only player when media inactive', () => {
    const a = createSubject()
    a.mediaActive = false
    a.navActive = true
    a.lastNavPlayerKey = 'nav-key'
    a.stopPlayerByKey = jest.fn()

    const before = Date.now()
    a.handleAudioData({ command: 8 })

    expect(a.navActive).toBe(false)
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('nav-key')
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.navHoldUntil).toBeGreaterThanOrEqual(before)
  })

  test('handleAudioData AudioOutputStop stops remembered players when no call or voiceAssistant is active', () => {
    const a = createSubject()
    a.lastMusicPlayerKey = 'music'
    a.lastNavPlayerKey = 'nav'
    a.lastVoiceAssistantPlayerKey = 'voiceAssistant'
    a.lastCallPlayerKey = 'call'
    a.stopPlayerByKey = jest.fn()

    a.handleAudioData({ command: 13 })

    expect(a.stopPlayerByKey).toHaveBeenCalledWith('music')
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('nav')
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('voiceAssistant')
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('call')
    expect(a.lastMusicPlayerKey).toBeNull()
    expect(a.lastNavPlayerKey).toBeNull()
    expect(a.lastVoiceAssistantPlayerKey).toBeNull()
    expect(a.lastCallPlayerKey).toBeNull()
  })

  test('handleAudioData AudioInputConfig updates current mic decode type', () => {
    const a = createSubject()

    a.handleAudioData({ command: 14, decodeType: 2 })

    expect(a.currentMicDecodeType).toBe(2)
  })

  test('handleAudioData AudioVoiceAssistantStart updates voiceAssistant state and skips mic start without decodeType', () => {
    const a = createSubject({ micType: 0, audioTransferMode: false })

    a.handleAudioData({ command: 4 })

    expect(a.voiceAssistantActive).toBe(true)
    expect(a.phonecallActive).toBe(false)
    expect(a.currentMicDecodeType).toBeNull()
  })

  test('handleAudioData AudioPhonecallStart updates phone state and stops mic in transfer mode', () => {
    const a = createSubject({ micType: 1, audioTransferMode: true })
    a._mic = { stop: jest.fn() }

    a.handleAudioData({ command: 15, decodeType: 1 })

    expect(a.phonecallActive).toBe(true)
    expect(a.voiceAssistantActive).toBe(false)
    expect(a._mic.stop).toHaveBeenCalled()
  })

  test('handleAudioData AudioVoiceAssistantStop clears state and stops player/mic', () => {
    const a = createSubject()
    a.voiceAssistantActive = true
    a.lastVoiceAssistantPlayerKey = 'va-key'
    a.stopPlayerByKey = jest.fn()
    a._mic = { stop: jest.fn() }

    a.handleAudioData({ command: 5 })

    expect(a.voiceAssistantActive).toBe(false)
    expect(a.stopPlayerByKey).toHaveBeenCalledWith('va-key')
    expect(a.lastVoiceAssistantPlayerKey).toBeNull()
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

  test('handleAudioData AudioAttentionStart sets uiCallIncoming and emits attention', () => {
    const emitAttention = jest.fn()
    const a = createSubject()
    a.emitAttention = emitAttention
    a.uiCallIncoming = false

    a.handleAudioData({ command: 1 }) // AudioAttentionStart

    expect(a.uiCallIncoming).toBe(true)
    expect(emitAttention).toHaveBeenCalledWith('call', true, { phase: 'incoming' })
  })

  test('handleAudioData AudioAttentionStart does not re-emit when uiCallIncoming already true', () => {
    const emitAttention = jest.fn()
    const a = createSubject()
    a.emitAttention = emitAttention
    a.uiCallIncoming = true

    a.handleAudioData({ command: 1 })

    expect(emitAttention).not.toHaveBeenCalled()
  })

  test('handleAudioData AudioAttentionRinging also sets uiCallIncoming', () => {
    const emitAttention = jest.fn()
    const a = createSubject()
    a.emitAttention = emitAttention
    a.uiCallIncoming = false

    a.handleAudioData({ command: 2 }) // AudioAttentionRinging

    expect(a.uiCallIncoming).toBe(true)
    expect(emitAttention).toHaveBeenCalledWith('call', true, { phase: 'incoming' })
  })

  test('handleAudioData AudioPhonecallStop emits attention ended when uiCallIncoming is true', () => {
    const emitAttention = jest.fn()
    const a = createSubject()
    a.emitAttention = emitAttention
    a.uiCallIncoming = true
    a._mic = { stop: jest.fn() }

    a.handleAudioData({ command: 3 }) // AudioPhonecallStop

    expect(a.uiCallIncoming).toBe(false)
    expect(emitAttention).toHaveBeenCalledWith('call', false, { phase: 'ended' })
  })

  test('handleAudioData AudioNaviStop emits attention nav:false when uiNavHintActive is true', () => {
    const emitAttention = jest.fn()
    const a = createSubject()
    a.emitAttention = emitAttention
    a.uiNavHintActive = true
    a.navActive = true
    a.lastNavPlayerKey = null
    a.stopPlayerByKey = jest.fn()

    a.handleAudioData({ command: 8 }) // AudioNaviStop

    expect(a.uiNavHintActive).toBe(false)
    expect(emitAttention).toHaveBeenCalledWith('nav', false)
  })

  test('handleAudioData AudioOutputStart does nothing when mediaActive is already true', () => {
    const a = createSubject()
    a.mediaActive = true
    a.audioOpenArmed = false

    a.handleAudioData({ command: 10 }) // AudioOutputStart

    // mediaActive stays true and audioOpenArmed remains unchanged
    expect(a.mediaActive).toBe(true)
    expect(a.audioOpenArmed).toBe(false)
  })

  test('handleAudioData AudioMediaStart returns early when audioOpenArmed and mediaActive both true', () => {
    const a = createSubject()
    a.audioOpenArmed = true
    a.mediaActive = true

    // Should return early at line 612 (mediaActive true inside audioOpenArmed branch)
    a.handleAudioData({ command: 11 }) // AudioMediaStart

    // mediaActive should still be true (unchanged)
    expect(a.mediaActive).toBe(true)
    expect(a.audioOpenArmed).toBe(true)
  })

  test('handleAudioData AudioNaviStop with mediaActive=true does not stop nav player', () => {
    const a = createSubject()
    a.navActive = true
    a.mediaActive = true // music still playing — let the OS sink drain nav tail naturally
    a.lastNavPlayerKey = 'nav-key'
    a.stopPlayerByKey = jest.fn()

    a.handleAudioData({ command: 8 }) // AudioNaviStop

    expect(a.navActive).toBe(false)
    // With mediaActive=true, stopPlayerByKey should NOT be called (else branch)
    expect(a.stopPlayerByKey).not.toHaveBeenCalled()
    expect(a.lastNavPlayerKey).toBe('nav-key')
  })

  test('handleAudioData AudioInputConfig restarts mic when decodeType changes and mic is capturing', () => {
    const a = createSubject()
    a.currentMicDecodeType = 1
    a._mic = { isCapturing: jest.fn(() => true), start: jest.fn(), stop: jest.fn() }

    a.handleAudioData({ command: 14, decodeType: 2 }) // decodeType changed from 1 to 2

    expect(a.currentMicDecodeType).toBe(2)
    expect(a._mic.start).toHaveBeenCalledWith(2)
  })

  test('handleAudioData AudioInputConfig does not restart mic when decodeType unchanged', () => {
    const a = createSubject()
    a.currentMicDecodeType = 2
    a._mic = { isCapturing: jest.fn(() => true), start: jest.fn(), stop: jest.fn() }

    a.handleAudioData({ command: 14, decodeType: 2 }) // same decodeType

    expect(a._mic.start).not.toHaveBeenCalled()
  })

  test('handleAudioData AudioVoiceAssistantStart with micType=0 creates mic and starts it with decodeType', () => {
    const { Microphone } = require('@main/services/audio')

    const a = createSubject({ micType: 0, audioTransferMode: false })
    a._mic = null

    a.handleAudioData({ command: 4, decodeType: 1 }) // AudioVoiceAssistantStart with decodeType

    expect(Microphone).toHaveBeenCalled()
    expect(a._mic).not.toBeNull()
    expect(a._mic.start).toHaveBeenCalledWith(1)
    expect(a.currentMicDecodeType).toBe(1)
  })

  test('handleAudioData AudioVoiceAssistantStart skips mic.start when no decodeType available', () => {
    const a = createSubject({ micType: 0, audioTransferMode: false })
    a._mic = null
    a.currentMicDecodeType = null

    a.handleAudioData({ command: 4 }) // AudioVoiceAssistantStart, no decodeType in msg

    expect(a.voiceAssistantActive).toBe(true)
    // mic is created but start is NOT called (no decode type)
    expect(a._mic).not.toBeNull()
    expect(a._mic.start).not.toHaveBeenCalled()
  })

  test('handleAudioData AudioVoiceAssistantStart reuses existing mic and sets decodeType from msg', () => {
    const existingMic = { on: jest.fn(), start: jest.fn(), stop: jest.fn(), isCapturing: jest.fn() }
    const a = createSubject({ micType: 0, audioTransferMode: false })
    a._mic = existingMic
    a.currentMicDecodeType = 1

    a.handleAudioData({ command: 4, decodeType: 2 })

    expect(a.currentMicDecodeType).toBe(2)
    expect(existingMic.start).toHaveBeenCalledWith(2)
  })

  test('handleAudioData with pcm data and visualizerEnabled sends chunked audio', () => {
    const sendChunked = jest.fn()
    const a = new (require('@main/services/projection/services/ProjectionAudio').ProjectionAudio)(
      () => ({ mediaDelay: 120 }) as any,
      jest.fn(),
      sendChunked,
      jest.fn()
    ) as any

    const player = { write: jest.fn() }
    a.getAudioOutputForStream = jest.fn(() => player)
    a.getLogicalStreamKey = jest.fn(() => 'music')
    a.mediaActive = true
    a.visualizerEnabled = true

    a.handleAudioData({ data: new Int16Array([1, 2, 3]), decodeType: 1 })

    const [, buf] = sendChunked.mock.calls[0]
    expect(Object.prototype.toString.call(buf)).toBe('[object ArrayBuffer]')
    expect(sendChunked).toHaveBeenCalledWith(
      'projection-audio-chunk',
      expect.anything(),
      64 * 1024,
      expect.objectContaining({ channels: 1 })
    )
  })

  test('stopAllAudioPlayers is called during reset and stops all players ignoring errors', () => {
    const a = createSubject()
    const throwingPlayer = {
      stop: jest.fn(() => {
        throw new Error('stop failed')
      })
    }
    const goodPlayer = { stop: jest.fn() }

    a.audioPlayers.set('48000:2', throwingPlayer)
    a.audioPlayers.set('16000:1', goodPlayer)

    // Should not throw even when player.stop() throws
    expect(() => a.resetForSessionStart()).not.toThrow()

    expect(throwingPlayer.stop).toHaveBeenCalled()
    expect(goodPlayer.stop).toHaveBeenCalled()
    expect(a.audioPlayers.size).toBe(0)
  })

  test('stopPlayerByKey swallows errors when player.stop throws', () => {
    const a = createSubject()
    const badPlayer = {
      stop: jest.fn(() => {
        throw new Error('stop error')
      })
    }
    a.audioPlayers.set('48000:2', badPlayer)

    expect(() => a.stopPlayerByKey('48000:2')).not.toThrow()
    expect(a.audioPlayers.size).toBe(0)
  })
})
