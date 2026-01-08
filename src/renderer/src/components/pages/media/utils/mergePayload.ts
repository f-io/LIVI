import { MediaPayload } from '../types'

export function mergePayload(prev: MediaPayload | undefined, inc: MediaPayload): MediaPayload {
  const prevMedia = prev?.media ?? {}
  const incMedia = inc.media ?? {}
  return {
    type: inc.type ?? prev?.type ?? 0,
    media:
      Object.keys(prevMedia).length || Object.keys(incMedia).length
        ? { ...prevMedia, ...incMedia }
        : undefined,
    base64Image: inc.base64Image !== undefined ? inc.base64Image : prev?.base64Image
  }
}
