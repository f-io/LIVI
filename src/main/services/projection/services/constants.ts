import { MediaType, NavigationMetaType } from '../messages'
import { PersistedMediaFile, PersistedNavigationFile } from './types'

export const APP_START_TS = Date.now()

export const DEFAULT_MEDIA_DATA_RESPONSE = {
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
      MediaPlayStatus: 0,
      MediaLyrics: '-'
    },
    error: true
  }
} satisfies PersistedMediaFile

export const DEFAULT_NAVIGATION_DATA_RESPONSE = {
  timestamp: '',
  payload: {
    metaType: NavigationMetaType.DashboardInfo,
    navi: null,
    rawUtf8: '',
    error: true
  }
} satisfies PersistedNavigationFile
