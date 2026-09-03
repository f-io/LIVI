/**
 * Audio sink channel, media (4), speech (5) or system (6). Setup and lifecycle live
 * here, the samples themselves go from the helper straight to the host.
 */

import { EventEmitter } from 'node:events'
import { AV_MSG, CH } from '../constants.js'
import type { RawFrame } from '../frame/codec.js'
import { decodeStart } from './protoEnc.js'

type SendFn = (channelId: number, flags: number, msgId: number, data: Buffer) => void

export type AudioChannelType = 'media' | 'speech' | 'system'

const CHANNEL_NAMES: Record<number, AudioChannelType> = {
  4: 'media',
  5: 'speech',
  6: 'system'
}

export class AudioChannel extends EventEmitter {
  // Events emitted:
  //   'setup' (codec: number, sampleRate: number, channels: number)           — format info
  //   'start' (channel: AudioChannelType, channelId: number)                  — START_INDICATION from phone
  //   'stop'  (channel: AudioChannelType, channelId: number)                  — STOP_INDICATION from phone

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
      case AV_MSG.START_INDICATION: {
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
}
