import * as audio from '@main/services/audio'

describe('audio index exports', () => {
  test('re-exports expected symbols', () => {
    expect(audio.AudioOutput).toBeDefined()
    expect(audio.Microphone).toBeDefined()
    expect(audio.downsampleToMono).toBeDefined()
  })
})
