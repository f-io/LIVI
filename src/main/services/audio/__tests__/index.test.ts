import * as audio from '@main/services/audio'

describe('audio index exports', () => {
  test('re-exports the host output and the system sounds, nothing else', () => {
    expect(audio.HostAudioOutput).toBeDefined()
    expect(audio.renderRelayClick).toBeDefined()
    expect(audio.SystemSound).toBeDefined()
    expect(Object.keys(audio).sort()).toEqual([
      'HostAudioOutput',
      'SystemSound',
      'renderRelayClick'
    ])
  })
})
