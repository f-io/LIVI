/**
 * Microphone channel handler, the control side of CH.MIC_INPUT (9). The samples
 * themselves go from the pipeline's tap to the helper, which sends them.
 *
 * Wire protocol:
 *   Phone → HU: SETUP_REQUEST (0x8000)
 *   HU → Phone: SETUP_RESPONSE (0x8003), accept setup
 *   Phone → HU: AV_INPUT_OPEN_REQUEST (0x8005), MicrophoneRequest{open:true/false}
 *   HU → Phone: AV_INPUT_OPEN_RESPONSE (0x8006), MicrophoneResponse{status, session_id}
 *   HU → Phone: START_INDICATION (0x8001), HU is the sender on input channels
 *   Phone → HU: AV_MEDIA_ACK (0x8004), flow control, answered by the helper's frames
 *   Phone → HU: STOP_INDICATION (0x8002), phone tears mic down
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, FRAME_FLAGS } from '../constants.js'
import type { RawFrame } from '../frame/codec.js'
import { decodeFields, decodeVarintValue, fieldVarint } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export class MicChannel extends EventEmitter {
  // Events:
  //   'mic-start' (channelId)  phone asked HU to begin sending mic PCM
  //   'mic-stop'  (channelId)  phone asked HU to stop / phone closed channel
  private _sampleRate = 16000
  private _channelCount = 1
  private _session = 1 // HU-chosen session id, echoed by phone in ACKs
  private _open = false // true between OPEN(open=true) and OPEN(open=false) / STOP

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
        // The phone confirms frames the helper sent
        break
      case AV_MSG.STOP_INDICATION:
        if (this._open) {
          this._open = false
          console.log(`[MicChannel] STOP_INDICATION, closing mic`)
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

  /** The capture format the phone negotiated at channel setup. */
  get format(): { sampleRate: number; channels: number } {
    return { sampleRate: this._sampleRate, channels: this._channelCount }
  }

  private _onOpenRequest(payload: Buffer): void {
    // MicrophoneRequest: f1 bool open, f2 anc, f3 ec, f4 max_unacked
    let open = false
    for (const f of decodeFields(payload)) {
      if (f.field === 1 && f.wire === 0) open = decodeVarintValue(f.bytes) !== 0
    }
    console.log(`[MicChannel] OPEN_REQUEST open=${open}`)
    // MicrophoneResponse: f1 status (0 = OK), f2 session_id
    const respBuf = Buffer.concat([fieldVarint(1, 0), fieldVarint(2, this._session)])
    this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.AV_INPUT_OPEN_RESPONSE, respBuf)
    if (open && !this._open) {
      this._open = true
      // HU-sent START_INDICATION on input channels: { session_id, configuration_index=0 }
      const startBuf = Buffer.concat([fieldVarint(1, this._session), fieldVarint(2, 0)])
      this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.START_INDICATION, startBuf)
      console.log(`[MicChannel] mic open, emitting mic-start, session=${this._session}`)
      this.emit('mic-start', this._channelId)
    } else if (!open && this._open) {
      this._open = false
      console.log(`[MicChannel] mic close, emitting mic-stop`)
      this.emit('mic-stop', this._channelId)
    }
  }
}
