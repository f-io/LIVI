import { PhoneType } from '@shared/types/Config'
import type { NaviInfo } from '@shared/types/NavigationTypes'
import type { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'

/**
 * Internal projection event model. Every driver (CarPlay, Android Auto,
 * dongle) emits these; wire parsing lives with the driver that owns the wire.
 */
export abstract class Message {}

export class DuckAudio extends Message {
  level: number
  durationMs: number

  constructor(level: number, durationMs: number) {
    super()
    this.level = Math.max(0, Math.min(1, level))
    this.durationMs = Math.max(0, durationMs)
  }
}

export class Command extends Message {
  value: CommandMapping

  constructor(value: CommandMapping) {
    super()
    this.value = value
  }
}

export { PhoneType }

export type AudioDataFields = {
  decodeType: number
  audioType: number
  command?: AudioCommand
}

/** An audio command of a driver, the samples themselves go from the helper to the host. */
export class AudioData extends Message {
  command?: AudioCommand
  decodeType: number
  audioType: number

  constructor(fields: AudioDataFields) {
    super()
    this.decodeType = fields.decodeType
    this.audioType = fields.audioType
    this.command = fields.command
  }
}

export enum MediaType {
  Data = 1,
  AlbumCover = 2,
  AlbumCoverAlt = 3,
  ControlAutoplayTrigger = 100
}

export enum NavigationMetaType {
  DashboardInfo = 200,
  DashboardImage = 201
}

export type MediaInfo = {
  MediaSongName?: string
  MediaAlbumName?: string
  MediaArtistName?: string
  MediaAPPName?: string
  MediaSongDuration?: number
  MediaSongPlayTime?: number
  MediaPlayStatus?: number
} & Record<string, unknown>

export type MediaPayload =
  | { type: MediaType.Data; media: MediaInfo }
  | { type: MediaType.AlbumCoverAlt; base64Image: string }
  | { type: MediaType.ControlAutoplayTrigger }

export class MediaData extends Message {
  mediaType: MediaType
  payload?: MediaPayload

  constructor(mediaType: MediaType, payload?: MediaPayload) {
    super()
    this.mediaType = mediaType
    this.payload = payload
  }
}

export type { NaviInfo } from '@shared/types/NavigationTypes'

export class NavigationData extends Message {
  metaType: NavigationMetaType
  navi: NaviInfo | null
  rawUtf8: string

  constructor(metaType: NavigationMetaType, navi: NaviInfo | null, rawUtf8 = '') {
    super()
    this.metaType = metaType
    this.navi = navi
    this.rawUtf8 = rawUtf8
  }
}
