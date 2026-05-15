import { MediaInfoChannel } from '../MediaInfoChannel'
import { fieldLenDelim, fieldVarint } from '../protoEnc'

const MEDIA_PLAYBACK_STATUS = 0x8001
const MEDIA_PLAYBACK_INPUT = 0x8002
const MEDIA_PLAYBACK_METADATA = 0x8003

describe('MediaInfoChannel — metadata', () => {
  test('decodes song/artist/album/playlist + duration + rating', () => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('metadata', cb)

    const payload = Buffer.concat([
      fieldLenDelim(1, Buffer.from('Hello World')),
      fieldLenDelim(2, Buffer.from('Artist')),
      fieldLenDelim(3, Buffer.from('Album')),
      fieldLenDelim(5, Buffer.from('My Playlist')),
      fieldVarint(6, 240),
      fieldVarint(7, 5)
    ])

    ch.handleMessage(MEDIA_PLAYBACK_METADATA, payload)
    expect(cb).toHaveBeenCalledWith({
      song: 'Hello World',
      artist: 'Artist',
      album: 'Album',
      playlist: 'My Playlist',
      durationSeconds: 240,
      rating: 5
    })
  })

  test('decodes album_art as a Buffer', () => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('metadata', cb)

    const art = Buffer.from([0xff, 0xd8, 0xff, 0xe0])
    ch.handleMessage(MEDIA_PLAYBACK_METADATA, fieldLenDelim(4, art))

    const m = cb.mock.calls[0][0]
    expect(Buffer.isBuffer(m.albumArt)).toBe(true)
    expect((m.albumArt as Buffer).equals(art)).toBe(true)
  })

  test('empty metadata payload emits an empty object', () => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('metadata', cb)
    ch.handleMessage(MEDIA_PLAYBACK_METADATA, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith({})
  })
})

describe('MediaInfoChannel — status', () => {
  test.each([
    [1, 'stopped'],
    [2, 'playing'],
    [3, 'paused'],
    [99, 'unknown']
  ])('state %s → %s', (raw, expected) => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('status', cb)
    ch.handleMessage(MEDIA_PLAYBACK_STATUS, fieldVarint(1, raw))
    expect(cb.mock.calls[0][0].state).toBe(expected)
  })

  test('decodes mediaSource + playbackSeconds + shuffle/repeat flags', () => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('status', cb)

    const payload = Buffer.concat([
      fieldVarint(1, 2), // playing
      fieldLenDelim(2, Buffer.from('Spotify')),
      fieldVarint(3, 42),
      fieldVarint(4, 1),
      fieldVarint(5, 1),
      fieldVarint(6, 0)
    ])
    ch.handleMessage(MEDIA_PLAYBACK_STATUS, payload)

    expect(cb).toHaveBeenCalledWith({
      state: 'playing',
      mediaSource: 'Spotify',
      playbackSeconds: 42,
      shuffle: true,
      repeat: true,
      repeatOne: false
    })
  })

  test('defaults state to "unknown" when no field 1', () => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('status', cb)
    ch.handleMessage(MEDIA_PLAYBACK_STATUS, fieldLenDelim(2, Buffer.from('Radio')))
    expect(cb.mock.calls[0][0]).toEqual({ state: 'unknown', mediaSource: 'Radio' })
  })
})

describe('MediaInfoChannel — passthrough behaviour', () => {
  test('MEDIA_PLAYBACK_INPUT is silently ignored', () => {
    const ch = new MediaInfoChannel()
    const cb = jest.fn()
    ch.on('metadata', cb)
    ch.on('status', cb)
    ch.handleMessage(MEDIA_PLAYBACK_INPUT, Buffer.alloc(0))
    expect(cb).not.toHaveBeenCalled()
  })

  test('unknown msgId is logged but does not throw', () => {
    const ch = new MediaInfoChannel()
    expect(() => ch.handleMessage(0xbeef, Buffer.from([1, 2, 3]))).not.toThrow()
  })
})
