import { EMPTY_STRING } from '../constants'
import { PersistedSnapshot } from '../types'

export const mediaProjectionOps = ({ snap }: { snap: PersistedSnapshot | null }) => {
  const media = snap?.payload.media
  const mediaPayloadError = snap?.payload.error
  const base64 = snap?.payload.base64Image
  const guessedMime = base64 && base64.startsWith('/9j/') ? 'image/jpeg' : 'image/png'
  const title = media?.MediaSongName ?? EMPTY_STRING
  const artist = media?.MediaArtistName ?? EMPTY_STRING
  const album = media?.MediaAlbumName ?? EMPTY_STRING
  const appName = media?.MediaAPPName ?? EMPTY_STRING
  const durationMs = media?.MediaSongDuration ?? 0
  const realPlaying = media?.MediaPlayStatus === 1
  const imageDataUrl = base64 ? `data:${guessedMime};base64,${base64}` : null

  return {
    media,
    mediaPayloadError,
    base64,
    guessedMime,
    title,
    artist,
    album,
    appName,
    durationMs,
    realPlaying,
    imageDataUrl
  }
}
