import { app } from 'electron'
import fs from 'fs'
import path from 'path'
import type { IPhoneDriver } from '../driver/IPhoneDriver'
import { type MediaData, MediaType, PhoneType } from '../messages'
import { DEFAULT_MEDIA_DATA_RESPONSE } from './constants'
import type { ProjectionSession } from './SessionManager'
import type { PersistedMediaPayload, ProjectionEvent } from './types'

export type MediaStoreDeps = {
  emit: (payload: ProjectionEvent) => void
  getPlaybackInferred: () => 1 | 2
  getLastPhoneType: () => PhoneType | undefined
}

// Persists and emits the phone's media (now-playing) snapshot, per session.
export class MediaStore {
  private readonly pending = new Map<IPhoneDriver, PersistedMediaPayload>()

  constructor(private readonly deps: MediaStoreDeps) {}

  private file(): string {
    return path.join(app.getPath('userData'), 'mediaData.json')
  }

  private write(payload: PersistedMediaPayload): void {
    const out = { timestamp: new Date().toISOString(), payload }
    fs.writeFileSync(this.file(), JSON.stringify(out, null, 2), 'utf8')
  }

  handle(
    driver: IPhoneDriver,
    session: ProjectionSession | null,
    msg: MediaData,
    isActive: boolean
  ): void {
    if (!msg.payload) return

    const existingPayload: PersistedMediaPayload =
      session?.media ?? this.pending.get(driver) ?? DEFAULT_MEDIA_DATA_RESPONSE.payload
    const newPayload: PersistedMediaPayload = { type: msg.payload.type }

    if (msg.payload.type === MediaType.Data && msg.payload.media) {
      const mergedMedia = { ...existingPayload.media, ...msg.payload.media }

      if (
        this.deps.getLastPhoneType() === PhoneType.AndroidAuto &&
        mergedMedia.MediaPlayStatus === undefined
      ) {
        mergedMedia.MediaPlayStatus = this.deps.getPlaybackInferred()
      }

      newPayload.media = mergedMedia
      if (existingPayload.base64Image) newPayload.base64Image = existingPayload.base64Image
    } else if ('base64Image' in msg.payload && msg.payload.base64Image) {
      newPayload.base64Image = msg.payload.base64Image
      if (existingPayload.media) newPayload.media = existingPayload.media
    } else {
      newPayload.media = existingPayload.media
      newPayload.base64Image = existingPayload.base64Image
    }

    if (session) {
      session.media = newPayload
      this.pending.delete(driver)
      if (isActive) {
        this.deps.emit({ type: 'media', payload: msg })
        this.write(newPayload)
      }
    } else {
      this.pending.set(driver, newPayload)
    }
  }

  patchAaPlayStatus(session: ProjectionSession | null, status: 1 | 2): void {
    if (!session) return
    try {
      const existingPayload: PersistedMediaPayload =
        session.media ?? DEFAULT_MEDIA_DATA_RESPONSE.payload

      const nextPayload: PersistedMediaPayload = {
        ...existingPayload,
        type: MediaType.Data,
        media: {
          ...existingPayload.media,
          MediaPlayStatus: status
        }
      }

      session.media = nextPayload
      this.write(nextPayload)

      this.deps.emit({
        type: 'media',
        payload: {
          mediaType: MediaType.Data,
          payload: {
            type: MediaType.Data,
            media: {
              MediaPlayStatus: status
            }
          }
        }
      })
    } catch (e) {
      console.warn('[MediaStore] patchAaPlayStatus failed (ignored)', e)
    }
  }

  hydrate(session: ProjectionSession): void {
    try {
      this.write(session.media ?? DEFAULT_MEDIA_DATA_RESPONSE.payload)
    } catch (e) {
      console.warn('[MediaStore] hydrate failed (ignored)', e)
    }

    this.deps.emit({ type: 'media-reset', reason: 'session-switch' })
  }

  reset(reason: string): void {
    try {
      this.write(DEFAULT_MEDIA_DATA_RESPONSE.payload)
    } catch (e) {
      console.warn('[MediaStore] reset failed (ignored)', reason, e)
    }

    this.deps.emit({ type: 'media-reset', reason })
  }
}
