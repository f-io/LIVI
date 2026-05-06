/**
 * Video channel handler — main display (CH.VIDEO=3) or cluster (CH.CLUSTER_VIDEO=19).
 *
 * Receives H.264/H.265 NAL units from the phone and emits them as 'frame' events.
 * Sends AVMediaAck for flow control.
 *
 * Wire protocol:
 *   Phone → HU: AV_MEDIA_INDICATION (0x0001) — H.264 data with timestamp
 *   Phone → HU: AV_MEDIA_WITH_TIMESTAMP (0x0000) — H.264 data (legacy)
 *   Phone → HU: SETUP_REQUEST (0x8000) — codec negotiation
 *   HU → Phone: SETUP_RESPONSE (0x8003) — accept setup
 *   HU → Phone: START_INDICATION (0x8001) — begin streaming
 *   HU → Phone: AV_MEDIA_ACK (0x8004) — flow control
 *
 * Field notes:
 *   - H.264 data already has AnnexB start codes (00 00 00 01). Do NOT add more.
 *   - SPS/PPS arrives as AV_MEDIA_INDICATION with no timestamp — forward to decoder first.
 *   - ACK every frame to avoid phone triggering CAR_NOT_RESPONDING (>400 unacked).
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, CH, FRAME_FLAGS } from '../constants.js'
import type { RawFrame } from '../frame/codec.js'
import { decodeStart, fieldVarint, readVarint } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export class VideoChannel extends EventEmitter {
  // Events emitted:
  //   'frame'  (h264: Buffer, timestamp: bigint)  — H.264 NAL unit, ready for decoder
  //   'setup'  (codec: number)                     — codec type from SETUP_REQUEST

  private _session = 0
  private _frameCount = 0
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
      case AV_MSG.AV_MEDIA_INDICATION:
        this._onMediaIndication(payload, false)
        break

      case AV_MSG.AV_MEDIA_WITH_TIMESTAMP:
        this._onMediaIndication(payload, true)
        break

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
        }
        break
      }

      default:
        console.debug(`[${this._label}] unhandled msgId=0x${msgId.toString(16)}`)
    }
  }

  private _onMediaIndication(payload: Buffer, hasTimestamp: boolean): void {
    let ts = 0n
    let data: Buffer

    if (hasTimestamp && payload.length >= 8) {
      // First 8 bytes = timestamp (uint64 nanoseconds, big-endian)
      ts = payload.readBigUInt64BE(0)
      data = payload.subarray(8)
    } else {
      ts = BigInt(Date.now()) * 1_000_000n
      data = payload
    }

    this._frameCount++
    this.emit('frame', data, ts)

    // Send AVMediaAck (required for flow control — phone disconnects if >400 unacked)
    this._sendAck()
  }

  private _sendAck(): void {
    // aap_protobuf.service.media.source.message.Ack:
    //   required int32  session_id           = 1;
    //   optional uint32 ack                  = 2;
    //   repeated uint64 receive_timestamp_ns = 3;
    const msgBuf = Buffer.concat([fieldVarint(1, this._session), fieldVarint(2, 1)])
    this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.AV_MEDIA_ACK, msgBuf)
  }

  get channelId(): number {
    return this._channelId
  }

  get frameCount(): number {
    return this._frameCount
  }
}
