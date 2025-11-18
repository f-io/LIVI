import { payloadFromLiveEvent } from '../../utils'
import type { MediaPayload } from '../../types'

describe('payloadFromLiveEvent', () => {
  it('returns null when event is undefined', () => {
    expect(payloadFromLiveEvent(undefined)).toBeNull()
  })

  it('returns null when event is not a media type', () => {
    const ev = { type: 'other', payload: { payload: { type: 1 } } }
    expect(payloadFromLiveEvent(ev)).toBeNull()
  })

  it('returns null when event payload is missing', () => {
    const ev = { type: 'media' }
    expect(payloadFromLiveEvent(ev)).toBeNull()
  })

  it('returns null when nested payload is missing', () => {
    const ev = { type: 'media', payload: {} }
    expect(payloadFromLiveEvent(ev)).toBeNull()
  })

  it('returns the inner MediaPayload when valid event is provided', () => {
    const payload: MediaPayload = {
      type: 1,
      media: { MediaSongName: 'Song', MediaArtistName: 'Artist' },
      base64Image: 'imgdata'
    }
    const ev = { type: 'media', payload: { payload } }

    expect(payloadFromLiveEvent(ev)).toEqual(payload)
  })

  it('handles extra fields gracefully (ignores them)', () => {
    const payload: MediaPayload = { type: 99 }
    const ev = {
      type: 'media',
      payload: { payload },
      extra: 'something',
      random: 42
    }

    expect(payloadFromLiveEvent(ev)).toEqual(payload)
  })
})
