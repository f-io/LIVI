import { ProjectionAudio } from '@main/services/projection/services/ProjectionAudio'

function createSubject() {
  return new ProjectionAudio(
    () => ({ mediaDelay: 120 }) as any,
    jest.fn(),
    jest.fn(),
    jest.fn()
  ) as any
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
})
