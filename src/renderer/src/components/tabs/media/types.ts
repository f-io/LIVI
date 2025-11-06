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
  error?: boolean
}

// USB/carplay event shape
export type UsbEvent = { type?: string } & Record<string, unknown>

export type MediaEventPayload = { type: 'media'; payload: { payload: MediaPayload } }

// Typed view of the pieces we use on window (no `any`)
export type Bridge = {
  carplay?: {
    ipc?: { onEvent?: (cb: (e: unknown, ...a: unknown[]) => void) => void | (() => void) }
  }
  electron?: {
    ipcRenderer?: {
      removeListener?: (channel: string, listener: (...a: unknown[]) => void) => void
    }
  }
}

export enum MediaEventType {
  PLAY = 'play',
  PAUSE = 'pause',
  STOP = 'stop',
  PREV = 'prev',
  NEXT = 'next'
}
