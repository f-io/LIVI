export type PersistedSnapshot = { timestamp: string; payload: MediaPayload }
export type MediaPayload = {
  type: number
  media?: {
    MediaSongName?: string
    MediaAlbumName?: string
    MediaArtistName?: string
    MediaAPPName?: string
    MediaSongDuration?: number
    MediaSongPlayTime?: number
    MediaPlayStatus?: number
    MediaLyrics?: string
  }
  base64Image?: string
}

// USB/carplay event shape
export type UsbEvent = { type?: string } & Record<string, unknown>

export type MediaEventPayload = { type: 'media'; payload: { payload: MediaPayload } }
