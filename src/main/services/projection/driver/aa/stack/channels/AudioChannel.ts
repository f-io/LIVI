/**
 * Audio channel handler (GAL types: MEDIA_AUDIO=4, SPEECH_AUDIO=5, PHONE_AUDIO=6).
 *
 * Receives PCM or AAC-LC frames from the phone and emits them as 'pcm' events.
 * Sends AVMediaAck for flow control (same as VideoChannel).
 *
 * Wire protocol (same as VideoChannel):
 *   Phone → HU: AV_MEDIA_INDICATION (0x0001) — audio data
 *   Phone → HU: AV_MEDIA_WITH_TIMESTAMP (0x0000) — audio data (legacy)
 *   Phone → HU: SETUP_REQUEST (0x8000) — codec negotiation
 *   HU → Phone: SETUP_RESPONSE (0x8003) — accept setup
 *   HU → Phone: START_INDICATION (0x8001) — begin streaming
 *   HU → Phone: AV_MEDIA_ACK (0x8004) — flow control
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, CH, FRAME_FLAGS } from '../constants.js'
import type { RawFrame } from '../frame/codec.js'
import { decodeStart, fieldVarint } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export type AudioChannelType = 'media' | 'speech' | 'phone'

const CHANNEL_NAMES: Record<number, AudioChannelType> = {
  4: 'media',
  5: 'speech',
  6: 'phone'
}

export class AudioChannel extends EventEmitter {
  // Events emitted:
  //   'pcm'   (samples: Buffer, timestamp: bigint, channel: AudioChannelType) — audio data
  //   'setup' (codec: number, sampleRate: number, channels: number)            — format info
  //   'start' (channel: AudioChannelType, channelId: number)                   — START_INDICATION from phone
  //   'stop'  (channel: AudioChannelType, channelId: number)                   — STOP_INDICATION from phone

  private _session = 0
  private _sampleRate = 48000
  private _channelCount = 2

  constructor(
    private readonly _channelId: number,
    private readonly _send: SendFn
  ) {
    super()
  }

  get channelType(): AudioChannelType {
    return CHANNEL_NAMES[this._channelId] ?? 'media'
  }

  handleMessage(msgId: number, payload: Buffer, _frame: RawFrame): void {
    switch (msgId) {
      case AV_MSG.AV_MEDIA_INDICATION:
        this._onMediaIndication(payload, false)
        break

      case AV_MSG.AV_MEDIA_WITH_TIMESTAMP:
        this._onMediaIndication(payload, true)
        break

      case AV_MSG.START_INDICATION: {
        // aap_protobuf.service.media.shared.message.Start { session_id=1, configuration_index=2 }.
        // Earlier this read the first 4 bytes as BE int32 — that decodes proto
        // wire bytes (0x08, varint, 0x10, varint) as a giant integer instead
        // of the actual session_id, so AVMediaAck would reference the wrong
        // session and the phone would silently drop audio frames.
        const start = decodeStart(payload)
        if (start) this._session = start.sessionId
        console.log(`[AudioChannel:${this.channelType}] stream started, session=${this._session}`)
        this.emit('start', this.channelType, this._channelId)
        break
      }

      case AV_MSG.STOP_INDICATION:
        console.log(`[AudioChannel:${this.channelType}] stream stopped`)
        this.emit('stop', this.channelType, this._channelId)
        break

      default:
        console.debug(`[AudioChannel:${this.channelType}] unhandled msgId=0x${msgId.toString(16)}`)
    }
  }

  /** Called by Session when AV setup arrives for this channel. */
  handleSetupRequest(codec: number, sampleRate: number, channelCount: number): void {
    this._sampleRate = sampleRate || this._sampleRate
    this._channelCount = channelCount || this._channelCount
    console.log(
      `[AudioChannel:${this.channelType}] setup codec=${codec} ` +
        `${this._sampleRate}Hz ${this._channelCount}ch`
    )
    this.emit('setup', codec, this._sampleRate, this._channelCount)
  }

  private _onMediaIndication(payload: Buffer, hasTimestamp: boolean): void {
    let ts: bigint
    let data: Buffer

    if (hasTimestamp && payload.length >= 8) {
      ts = payload.readBigUInt64BE(0)
      data = payload.subarray(8)
    } else {
      ts = BigInt(Date.now()) * 1_000_000n
      data = payload
    }

    this.emit('pcm', data, ts, this.channelType)
    this._sendAck()
  }

  private _sendAck(): void {
    // aap_protobuf.service.media.source.message.Ack — see VideoChannel._sendAck
    // for the rationale on the canonical (session_id + ack=1) form.
    const msgBuf = Buffer.concat([fieldVarint(1, this._session), fieldVarint(2, 1)])
    this._send(this._channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.AV_MEDIA_ACK, msgBuf)
  }
}
