/**
 * Microphone channel handler — outbound counterpart of AudioChannel.
 *
 * Wire protocol on CH.MIC_INPUT (=9):
 *   Phone → HU: SETUP_REQUEST (0x8000)
 *   HU → Phone: SETUP_RESPONSE (0x8003) — accept setup
 *   Phone → HU: AV_INPUT_OPEN_REQUEST (0x8005) — MicrophoneRequest{open:true/false}
 *   HU → Phone: AV_INPUT_OPEN_RESPONSE (0x8006) — MicrophoneResponse{status, session_id}
 *   HU → Phone: START_INDICATION (0x8001) — HU is the sender on input channels
 *   HU → Phone: AV_MEDIA_INDICATION (0x0001) — mic PCM frames (with timestamp)
 *   Phone → HU: AV_MEDIA_ACK (0x8004) — flow control
 *   Phone → HU: STOP_INDICATION (0x8002) — phone tears mic down
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, FRAME_FLAGS } from '../constants.js'
import type { RawFrame } from '../frame/codec.js'
import { decodeFields, decodeVarintValue, fieldVarint } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export class MicChannel extends EventEmitter {
  // Events:
  //   'mic-start' (channelId)  — phone asked HU to begin sending mic PCM
  //   'mic-stop'  (channelId)  — phone asked HU to stop / phone closed channel

  private _sampleRate = 16000
  private _channelCount = 1
  private _session = 1 // HU-chosen session id, echoed by phone in ACKs
  private _open = false // true between OPEN(open=true) and OPEN(open=false) / STOP
  private _maxUnacked = 1
  private _unacked = 0 // frames sent but not yet acked
  private _pending: { ts: bigint; data: Buffer }[] = [] // backlog while unacked >= max

  constructor(
    private readonly _channelId: number,
    private readonly _send: SendFn
  ) {
    super()
  }

  handleMessage(msgId: number, payload: Buffer, _frame: RawFrame): void {
    switch (msgId) {
      case AV_MSG.SETUP_REQUEST:
        // Setup request on mic channel
        break

      case AV_MSG.AV_INPUT_OPEN_REQUEST:
        this._onOpenRequest(payload)
        break

      case AV_MSG.AV_MEDIA_ACK:
        // Flow control — phone confirms it received N frames
        if (this._unacked > 0) this._unacked -= 1
        this._drainPending()
        break

      case AV_MSG.STOP_INDICATION:
        if (this._open) {
          this._open = false
          this._unacked = 0
          this._pending.length = 0
          console.log(`[MicChannel] STOP_INDICATION — closing mic`)
          this.emit('mic-stop', this._channelId)
        }
        break

      default:
        console.debug(`[MicChannel] unhandled msgId=0x${msgId.toString(16)}`)
    }
  }

  /** Called by Session when phone's AVChannelSetupRequest arrives. */
  handleSetupRequest(codec: number, sampleRate: number, channelCount: number): void {
    this._sampleRate = sampleRate || this._sampleRate
    this._channelCount = channelCount || this._channelCount
    console.log(`[MicChannel] setup codec=${codec} ${this._sampleRate}Hz ${this._channelCount}ch`)
  }

  /**
   * Push a PCM chunk to the phone. Wraps in AV_MEDIA_INDICATION with the
   * timestamp prefix the phone-side decoder expects.
   */
  pushPcm(buf: Buffer, ts: bigint): void {
    if (!this._open) return
    if (this._unacked >= this._maxUnacked) {
      this._pending.push({ ts, data: buf })
      if (this._pending.length > 64) this._pending.shift()
      return
    }
    this._sendFrame(buf, ts)
  }

  private _drainPending(): void {
    while (this._pending.length > 0 && this._unacked < this._maxUnacked) {
      const next = this._pending.shift()!
      this._sendFrame(next.data, next.ts)
    }
  }

  private _sendFrame(buf: Buffer, ts: bigint): void {
    // AV_MEDIA_WITH_TIMESTAMP layout: 8-byte BE timestamp + raw PCM samples.
    const out = Buffer.allocUnsafe(8 + buf.length)
    out.writeBigUInt64BE(ts, 0)
    buf.copy(out, 8)
    this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.AV_MEDIA_WITH_TIMESTAMP, out)
    this._unacked += 1
  }

  private _onOpenRequest(payload: Buffer): void {
    // MicrophoneRequest: f1 bool open, f2 anc, f3 ec, f4 max_unacked
    let open = false
    for (const f of decodeFields(payload)) {
      if (f.field === 1 && f.wire === 0) open = decodeVarintValue(f.bytes) !== 0
      else if (f.field === 4 && f.wire === 0)
        this._maxUnacked = Math.max(1, decodeVarintValue(f.bytes))
    }
    console.log(`[MicChannel] OPEN_REQUEST open=${open} maxUnacked=${this._maxUnacked}`)

    // MicrophoneResponse: f1 status (0 = OK), f2 session_id
    const respBuf = Buffer.concat([fieldVarint(1, 0), fieldVarint(2, this._session)])
    this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.AV_INPUT_OPEN_RESPONSE, respBuf)

    if (open && !this._open) {
      this._open = true
      this._unacked = 0
      this._pending.length = 0

      // HU-sent START_INDICATION on input channels: { session_id, configuration_index=0 }
      const startBuf = Buffer.concat([fieldVarint(1, this._session), fieldVarint(2, 0)])
      this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.START_INDICATION, startBuf)

      console.log(`[MicChannel] mic open — emitting mic-start, session=${this._session}`)
      this.emit('mic-start', this._channelId)
    } else if (!open && this._open) {
      this._open = false
      this._unacked = 0
      this._pending.length = 0
      console.log(`[MicChannel] mic close — emitting mic-stop`)
      this.emit('mic-stop', this._channelId)
    }
  }
}
