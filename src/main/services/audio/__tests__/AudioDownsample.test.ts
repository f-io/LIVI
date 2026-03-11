import { downsampleToMono } from '@main/services/audio/AudioDownsample'

describe('downsampleToMono', () => {
  test('returns empty array for empty pcm', () => {
    const out = downsampleToMono(new Int16Array(0), { inSampleRate: 48000, inChannels: 2 })
    expect(out).toEqual(new Int16Array(0))
  })

  test('returns input as-is for mono and same sample rate', () => {
    const pcm = new Int16Array([1, 2, 3])
    const out = downsampleToMono(pcm, { inSampleRate: 48000, inChannels: 1, outSampleRate: 48000 })
    expect(out).toBe(pcm)
  })

  test('mixes stereo to mono and downsamples by ratio', () => {
    const pcm = new Int16Array([10, 30, 20, 40, 30, 50, 40, 60])
    const out = downsampleToMono(pcm, { inSampleRate: 48000, inChannels: 2, outSampleRate: 24000 })
    expect(Array.from(out)).toEqual([20, 40])
  })

  test('returns empty for invalid channels or ratio', () => {
    expect(
      downsampleToMono(new Int16Array([1, 2]), {
        inSampleRate: 48000,
        inChannels: 0
      })
    ).toEqual(new Int16Array(0))

    expect(
      downsampleToMono(new Int16Array([1, 2]), {
        inSampleRate: 0,
        inChannels: 1,
        outSampleRate: 16000
      })
    ).toEqual(new Int16Array(0))
  })
})
