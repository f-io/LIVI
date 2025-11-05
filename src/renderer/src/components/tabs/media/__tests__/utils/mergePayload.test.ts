import { mergePayload } from '../../utils'
import { MediaPayload } from '../../types'

describe('mergePayload', () => {
  it('returns incoming payload when previous is undefined', () => {
    const inc: MediaPayload = {
      type: 1,
      media: { MediaSongName: 'Song', MediaArtistName: 'Artist' },
      base64Image: 'image-data'
    }

    expect(mergePayload(undefined, inc)).toEqual(inc)
  })

  it('merges media objects correctly', () => {
    const prev: MediaPayload = {
      type: 1,
      media: {
        MediaSongName: 'Old Song',
        MediaArtistName: 'Old Artist',
        MediaAlbumName: 'Old Album'
      },
      base64Image: 'old-image'
    }

    const inc: MediaPayload = {
      type: 2,
      media: { MediaArtistName: 'New Artist' }
    }

    expect(mergePayload(prev, inc)).toEqual({
      type: 2,
      media: {
        MediaSongName: 'Old Song',
        MediaArtistName: 'New Artist',
        MediaAlbumName: 'Old Album'
      },
      base64Image: 'old-image'
    })
  })

  it('keeps previous type if incoming type is undefined', () => {
    const prev: MediaPayload = { type: 3, media: { MediaSongName: 'Track A' } }
    const inc: MediaPayload = { type: undefined as never, media: { MediaArtistName: 'B' } }

    expect(mergePayload(prev, inc).type).toBe(3)
  })

  it('sets media to undefined when both are empty', () => {
    const prev: MediaPayload = { type: 1, media: {} }
    const inc: MediaPayload = { type: 2, media: {} }

    expect(mergePayload(prev, inc).media).toBeUndefined()
  })

  it('uses incoming base64Image if provided (even null)', () => {
    const prev: MediaPayload = { type: 1, media: {}, base64Image: 'old' }
    const inc: MediaPayload = { type: 1, media: {}, base64Image: null as never }

    expect(mergePayload(prev, inc).base64Image).toBeNull()
  })

  it('keeps previous base64Image if incoming is undefined', () => {
    const prev: MediaPayload = { type: 1, media: {}, base64Image: 'old' }
    const inc: MediaPayload = { type: 1, media: {} }

    expect(mergePayload(prev, inc).base64Image).toBe('old')
  })
})
