/**
 * Media playback status channel handler (CH.MEDIA_INFO = 13).
 *
 * Phone → HU only (we don't drive playback metadata, we just receive it).
 *
 * Wire protocol (aap_protobuf.service.mediaplayback):
 *   MEDIA_PLAYBACK_STATUS    = 32769  → MediaPlaybackStatus { state, media_source, position, shuffle/repeat }
 *   MEDIA_PLAYBACK_INPUT     = 32770  → playback control (HU → Phone, not implemented here)
 *   MEDIA_PLAYBACK_METADATA  = 32771  → MediaPlaybackMetadata { song, artist, album, album_art, … }
 *
 * Proto schemas (relevant fields):
 *
 *   message MediaPlaybackStatus {
 *     optional State  state            = 1;   // STOPPED=1, PLAYING=2, PAUSED=3
 *     optional string media_source     = 2;
 *     optional uint32 playback_seconds = 3;
 *     optional bool   shuffle          = 4;
 *     optional bool   repeat           = 5;
 *     optional bool   repeat_one       = 6;
 *   }
 *
 *   message MediaPlaybackMetadata {
 *     optional string song              = 1;
 *     optional string artist            = 2;
 *     optional string album             = 3;
 *     optional bytes  album_art         = 4;
 *     optional string playlist          = 5;
 *     optional uint32 duration_seconds  = 6;
 *     optional int32  rating            = 7;
 *   }
 *
 * Channel-open response is handled by Session._handleDecryptedMessage's
 * generic CHANNEL_OPEN_REQUEST path; nothing channel-specific to do there.
 */

import { EventEmitter } from 'node:events'
import { decodeFields, decodeVarintValue } from './protoEnc.js'

const MEDIA_MSG = {
  MEDIA_PLAYBACK_STATUS: 0x8001, // = 32769
  MEDIA_PLAYBACK_INPUT: 0x8002, // = 32770
  MEDIA_PLAYBACK_METADATA: 0x8003 // = 32771
} as const

export interface MediaPlaybackMetadata {
  song?: string
  artist?: string
  album?: string
  playlist?: string
  durationSeconds?: number
  rating?: number
  /** Raw album-art bytes (typically JPEG/PNG) — pass through to UI as-is. */
  albumArt?: Buffer
}

export type MediaPlaybackState = 'stopped' | 'playing' | 'paused' | 'unknown'

export interface MediaPlaybackStatus {
  state: MediaPlaybackState
  mediaSource?: string
  playbackSeconds?: number
  shuffle?: boolean
  repeat?: boolean
  repeatOne?: boolean
}

export class MediaInfoChannel extends EventEmitter {
  // Events emitted:
  //   'metadata' (m: MediaPlaybackMetadata) — track info from the phone
  //   'status'   (s: MediaPlaybackStatus)   — playback state / position update

  handleMessage(msgId: number, payload: Buffer): void {
    switch (msgId) {
      case MEDIA_MSG.MEDIA_PLAYBACK_METADATA: {
        const m = this._decodeMetadata(payload)
        // Compact log so we can see which fields the phone actually sends —
        // some apps omit duration_seconds (Spotify Free), some never send
        // album_art (radio streams). Length-only for art so the line stays short.
        console.log(
          `[MediaInfoChannel] metadata: song=${JSON.stringify(m.song)} artist=${JSON.stringify(m.artist)}` +
            ` album=${JSON.stringify(m.album)} duration=${m.durationSeconds}s` +
            ` art=${m.albumArt ? `${m.albumArt.length}B` : 'none'} playlist=${JSON.stringify(m.playlist)}`
        )
        this.emit('metadata', m)
        break
      }

      case MEDIA_MSG.MEDIA_PLAYBACK_STATUS: {
        const s = this._decodeStatus(payload)
        console.log(
          `[MediaInfoChannel] status: state=${s.state} pos=${s.playbackSeconds}s` +
            ` source=${JSON.stringify(s.mediaSource)} shuffle=${s.shuffle} repeat=${s.repeat}`
        )
        this.emit('status', s)
        break
      }

      case MEDIA_MSG.MEDIA_PLAYBACK_INPUT:
        // HU→Phone direction in aasdk; if the phone ever echoes one back to
        // us we just ignore it — playback control is one-way from us.
        console.debug('[MediaInfoChannel] MEDIA_PLAYBACK_INPUT echoed — ignored')
        break

      default:
        // Log once per unknown msgId so we don't flood if the phone keeps
        // pushing something we don't recognise.
        console.log(
          `[MediaInfoChannel] unhandled msgId=0x${msgId.toString(16)} len=${payload.length} hex=${payload.toString('hex').slice(0, 80)}`
        )
    }
  }

  private _decodeMetadata(payload: Buffer): MediaPlaybackMetadata {
    const out: MediaPlaybackMetadata = {}
    for (const f of decodeFields(payload)) {
      switch (f.field) {
        case 1:
          out.song = f.bytes.toString('utf8')
          break
        case 2:
          out.artist = f.bytes.toString('utf8')
          break
        case 3:
          out.album = f.bytes.toString('utf8')
          break
        case 4:
          out.albumArt = Buffer.from(f.bytes)
          break
        case 5:
          out.playlist = f.bytes.toString('utf8')
          break
        case 6:
          out.durationSeconds = decodeVarintValue(f.bytes)
          break
        case 7:
          out.rating = decodeVarintValue(f.bytes)
          break
      }
    }
    return out
  }

  private _decodeStatus(payload: Buffer): MediaPlaybackStatus {
    const out: MediaPlaybackStatus = { state: 'unknown' }
    for (const f of decodeFields(payload)) {
      switch (f.field) {
        case 1: {
          const v = decodeVarintValue(f.bytes)
          out.state = v === 1 ? 'stopped' : v === 2 ? 'playing' : v === 3 ? 'paused' : 'unknown'
          break
        }
        case 2:
          out.mediaSource = f.bytes.toString('utf8')
          break
        case 3:
          out.playbackSeconds = decodeVarintValue(f.bytes)
          break
        case 4:
          out.shuffle = decodeVarintValue(f.bytes) !== 0
          break
        case 5:
          out.repeat = decodeVarintValue(f.bytes) !== 0
          break
        case 6:
          out.repeatOne = decodeVarintValue(f.bytes) !== 0
          break
      }
    }
    return out
  }
}
