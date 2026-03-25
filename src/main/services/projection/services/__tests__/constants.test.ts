import { MediaType, NavigationMetaType } from '../../messages'
import {
  APP_START_TS,
  DEFAULT_MEDIA_COVER_IMAGE,
  DEFAULT_MEDIA_DATA_RESPONSE,
  DEFAULT_NAVIGATION_DATA_RESPONSE
} from '../constants'

describe('projection service constants', () => {
  test('APP_START_TS is initialized as a number timestamp', () => {
    expect(typeof APP_START_TS).toBe('number')
    expect(Number.isFinite(APP_START_TS)).toBe(true)
    expect(APP_START_TS).toBeGreaterThan(0)
  })

  test('DEFAULT_MEDIA_COVER_IMAGE is a non-empty base64 string', () => {
    expect(typeof DEFAULT_MEDIA_COVER_IMAGE).toBe('string')
    expect(DEFAULT_MEDIA_COVER_IMAGE.length).toBeGreaterThan(0)
    expect(DEFAULT_MEDIA_COVER_IMAGE.startsWith('iVBOR')).toBe(true)
  })

  test('DEFAULT_MEDIA_DATA_RESPONSE contains the expected default media payload', () => {
    expect(DEFAULT_MEDIA_DATA_RESPONSE).toEqual({
      timestamp: '',
      payload: {
        type: MediaType.Data,
        media: {
          MediaSongName: '-',
          MediaAlbumName: '-',
          MediaArtistName: '-',
          MediaAPPName: '-',
          MediaSongDuration: 0,
          MediaSongPlayTime: 0,
          MediaPlayStatus: 1,
          MediaLyrics: '-'
        },
        base64Image: DEFAULT_MEDIA_COVER_IMAGE,
        error: true
      }
    })
  })

  test('DEFAULT_NAVIGATION_DATA_RESPONSE contains the expected default navigation payload', () => {
    expect(DEFAULT_NAVIGATION_DATA_RESPONSE).toEqual({
      timestamp: '',
      payload: {
        metaType: NavigationMetaType.DashboardInfo,
        navi: null,
        rawUtf8: '',
        error: true
      }
    })
  })

  test('default response objects expose error states for fallback handling', () => {
    expect(DEFAULT_MEDIA_DATA_RESPONSE.payload.error).toBe(true)
    expect(DEFAULT_NAVIGATION_DATA_RESPONSE.payload.error).toBe(true)
  })

  test('default media response uses the shared default cover image constant', () => {
    expect(DEFAULT_MEDIA_DATA_RESPONSE.payload.base64Image).toBe(DEFAULT_MEDIA_COVER_IMAGE)
  })
})
