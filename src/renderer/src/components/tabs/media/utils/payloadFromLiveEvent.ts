import { MediaEventPayload, MediaPayload } from '../types'

export function payloadFromLiveEvent(ev: unknown): MediaPayload | null {
  const e = ev as Partial<MediaEventPayload>
  if (e?.type !== 'media' || !e.payload?.payload) return null
  return e.payload.payload
}
