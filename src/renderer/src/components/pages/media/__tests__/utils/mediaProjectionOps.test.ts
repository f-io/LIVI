import { EMPTY_STRING } from '../../constants'
import { mediaProjectionOps } from '../../utils/mediaProjectionOps'

describe('mediaProjectionOps', () => {
  test('returns defaults when snapshot is null', () => {
    const result = mediaProjectionOps({ snap: null })

    expect(result).toEqual({
      media: undefined,
      mediaPayloadError: undefined,
      base64: undefined,
      guessedMime: 'image/png',
      title: EMPTY_STRING,
      artist: EMPTY_STRING,
      album: EMPTY_STRING,
      appName: EMPTY_STRING,
      durationMs: 0,
      realPlaying: false,
      imageDataUrl: null
    })
  })

  test('uses jpeg mime and populated media fields when jpeg base64 is present', () => {
    const result = mediaProjectionOps({
      snap: {
        payload: {
          media: {
            MediaSongName: 'Song',
            MediaArtistName: 'Artist',
            MediaAlbumName: 'Album',
            MediaAPPName: 'App',
            MediaSongDuration: 1234,
            MediaPlayStatus: 1
          },
          error: 'some-error',
          base64Image: '/9j/abc123'
        }
      } as any
    })

    expect(result.mediaPayloadError).toBe('some-error')
    expect(result.base64).toBe('/9j/abc123')
    expect(result.guessedMime).toBe('image/jpeg')
    expect(result.title).toBe('Song')
    expect(result.artist).toBe('Artist')
    expect(result.album).toBe('Album')
    expect(result.appName).toBe('App')
    expect(result.durationMs).toBe(1234)
    expect(result.realPlaying).toBe(true)
    expect(result.imageDataUrl).toBe('data:image/jpeg;base64,/9j/abc123')
  })

  test('uses png mime when base64 is present but not jpeg and falls back for missing media fields', () => {
    const result = mediaProjectionOps({
      snap: {
        payload: {
          media: {
            MediaPlayStatus: 0
          },
          base64Image: 'iVBORw0KGgoAAAANSUhEUgAA'
        }
      } as any
    })

    expect(result.guessedMime).toBe('image/png')
    expect(result.title).toBe(EMPTY_STRING)
    expect(result.artist).toBe(EMPTY_STRING)
    expect(result.album).toBe(EMPTY_STRING)
    expect(result.appName).toBe(EMPTY_STRING)
    expect(result.durationMs).toBe(0)
    expect(result.realPlaying).toBe(false)
    expect(result.imageDataUrl).toBe('data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAA')
  })
})
