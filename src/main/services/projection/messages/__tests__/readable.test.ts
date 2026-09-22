import { DuckAudio, MediaData } from '@main/services/projection/messages'

describe('readable messages', () => {
  test('MediaData keeps unknown media type payload undefined', () => {
    const msg = new MediaData(999 as any)

    expect(msg.mediaType).toBe(999)
    expect(msg.payload).toBeUndefined()
  })

  test('DuckAudio clamps level to [0,1] and duration to >= 0', () => {
    const msg = new DuckAudio(0.5, 200)
    expect(msg.level).toBe(0.5)
    expect(msg.durationMs).toBe(200)

    const clamped = new DuckAudio(2, -50)
    expect(clamped.level).toBe(1)
    expect(clamped.durationMs).toBe(0)

    expect(new DuckAudio(-1, 0).level).toBe(0)
  })
})
