import { MediaPayload } from '../types'

export function mergePayload(prev: MediaPayload | undefined, inc: MediaPayload): MediaPayload {
  return {
    type: inc.type ?? prev?.type ?? 0,
    media: inc.media,
    base64Image: inc.base64Image !== undefined ? inc.base64Image : prev?.base64Image
  }
}
