/**
 * Video channel, main display (CH.VIDEO) or cluster (CH.CLUSTER_VIDEO). Setup, focus and
 * lifecycle live here, the frames themselves go from the helper straight to the host.
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, CH, FRAME_FLAGS } from '../constants.js'
import type { RawFrame } from '../frame/codec.js'
import { decodeStart, fieldVarint, readVarint } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export class VideoChannel extends EventEmitter {
  // Events: 'setup' (codec), 'host-ui-requested', 'video-focus-projected'

  private _session = 0
  private readonly _channelId: number
  private readonly _label: string

  constructor(
    private readonly _send: SendFn,
    channelId: number = CH.VIDEO
  ) {
    super()
    this._channelId = channelId
    this._label = channelId === CH.CLUSTER_VIDEO ? 'ClusterVideoChannel' : 'VideoChannel'
  }

  handleMessage(msgId: number, payload: Buffer, frame: RawFrame): void {
    switch (msgId) {
      case AV_MSG.START_INDICATION: {
        // aap_protobuf.service.media.shared.message.Start { session_id=1, configuration_index=2 }.
        // Was previously read as `payload.readInt32BE(0)` which returns the
        // first 4 wire bytes (`0x08, varint, 0x10, varint`) interpreted as a
        // big-endian int32 — never the actual session_id. The phone tolerated
        // it because AVMediaAck.session_id is an `int32` proto field that the
        // phone doesn't strictly validate against its own session counter,
        // but we should still send the correct value.
        const start = decodeStart(payload)
        if (start) this._session = start.sessionId
        console.log(`[${this._label}] stream started, session=${this._session}`)
        break
      }

      case AV_MSG.STOP_INDICATION:
        console.log(`[${this._label}] stream stopped`)
        break

      case AV_MSG.VIDEO_FOCUS_INDICATION:
        // Phone granted/revoked video focus — nothing to do for passthrough
        console.debug(`[${this._label}] VideoFocusIndication`)
        break

      case AV_MSG.VIDEO_FOCUS_REQUEST: {
        // VideoFocusRequestNotification {
        //   optional int32 disp_channel_id = 1 [deprecated];
        //   optional VideoFocusMode mode = 2;     // PROJECTED=1, NATIVE=2, NATIVE_TRANSIENT=3
        //   optional VideoFocusReason reason = 3; // UNKNOWN=0, PHONE_SCREEN_OFF=1, LAUNCH_NATIVE=2
        // }
        let mode = 1 // default PROJECTED if missing
        let off = 0
        while (off < payload.length) {
          const t = payload[off++]!
          if (t === 0x10) {
            // field 2 (mode), varint
            const [v, n] = readVarint(payload, off)
            mode = v
            off += n
          } else {
            // unknown / deprecated field — skip the varint payload
            const [, n] = readVarint(payload, off)
            off += n
          }
        }
        const modeName = mode === 2 ? 'NATIVE' : mode === 3 ? 'NATIVE_TRANSIENT' : 'PROJECTED'
        console.log(
          `[${this._label}] VideoFocusRequest mode=${modeName}(${mode}) → responding PROJECTED`
        )
        this._send(
          this._channelId,
          FRAME_FLAGS.ENC_SIGNAL,
          AV_MSG.VIDEO_FOCUS_INDICATION,
          Buffer.from([0x08, 0x01])
        )
        if (mode === 2 || mode === 3) {
          // NATIVE / NATIVE_TRANSIENT — user wants the host UI
          this.emit('host-ui-requested')
        } else {
          this.emit('video-focus-projected')
        }
        break
      }

      default:
        console.debug(`[${this._label}] unhandled msgId=0x${msgId.toString(16)}`)
    }
  }

  get channelId(): number {
    return this._channelId
  }
}
