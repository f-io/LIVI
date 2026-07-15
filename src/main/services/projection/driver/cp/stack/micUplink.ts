import dgram from 'node:dgram'
import { chachaSeal } from './crypto'
import { CpMicUplinkEncoder } from './micUplinkEncoder'

const RTP_HEADER_LEN = 12

export interface CpMicUplinkOpts {
  key: Buffer
  host: string
  port: number
  sampleRate: number
  channels: number
  /** RTP payload type = the CarPlay stream type. */
  payloadType: number
  /** 'pcm' sends captured PCM as-is (wired); 'opus' encodes it first (wireless). */
  codec: 'pcm' | 'opus'
  /** OPUS frame duration in ms; also sets the RTP timestamp step. */
  frameMs: number
  /** OPUS target bitrate (rate-tiered per R6). */
  bitrate: number
  label: string
}

export class CpMicUplink {
  private _sock: dgram.Socket | null = null
  private _seq = 0
  private _ts = 0
  private _nonce = 0n
  private _pcm = Buffer.alloc(0)
  private _enc: CpMicUplinkEncoder | null = null
  private readonly _samplesPerFrame: number
  private readonly _frameBytes: number

  constructor(private readonly opts: CpMicUplinkOpts) {
    this._samplesPerFrame = Math.round((opts.sampleRate * opts.frameMs) / 1000)
    this._frameBytes = this._samplesPerFrame * opts.channels * 2
  }

  start(): void {
    if (this._sock) return
    this._sock = dgram.createSocket({ type: 'udp6', ipv6Only: false })
    this._sock.on('error', () => {})
    this._sock.bind(0, '::')
    if (this.opts.codec === 'opus') {
      const enc = new CpMicUplinkEncoder({
        sampleRate: this.opts.sampleRate,
        channels: this.opts.channels,
        bitrate: this.opts.bitrate,
        frameMs: this.opts.frameMs,
        label: this.opts.label
      })
      enc.on('opus', (frame: Buffer) => this._send(frame, false))
      this._enc = enc
      enc
        .start()
        .catch((e: Error) =>
          console.warn(`[cpMicUplink:${this.opts.label}] encoder start failed: ${e.message}`)
        )
    }
  }

  write(pcmLe: Buffer): void {
    if (!this._sock) return
    if (this._enc) {
      this._enc.write(pcmLe)
      return
    }
    // PCM passthrough: frame at the fixed samples-per-packet and send each raw.
    this._pcm = Buffer.concat([this._pcm, pcmLe])
    while (this._pcm.length >= this._frameBytes) {
      const frame = Buffer.from(this._pcm.subarray(0, this._frameBytes))
      this._pcm = this._pcm.subarray(this._frameBytes)
      this._send(frame, true)
    }
  }

  private _send(payload: Buffer, isPcm: boolean): void {
    const sock = this._sock
    if (!sock) return
    // PCM travels big-endian on the wire; OPUS frames go as-is.
    const body = isPcm ? payload.swap16() : payload
    const header = Buffer.allocUnsafe(RTP_HEADER_LEN)
    header[0] = 0x80
    header[1] = this.opts.payloadType & 0x7f
    header.writeUInt16BE(this._seq & 0xffff, 2)
    header.writeUInt32BE(this._ts >>> 0, 4)
    // CarPlay input streams use SSRC 0 (per the R19 reference).
    header.writeUInt32BE(0, 8)
    const aad = header.subarray(4, RTP_HEADER_LEN)
    const nonce8 = Buffer.allocUnsafe(8)
    nonce8.writeBigUInt64LE(this._nonce)
    const nonce12 = Buffer.concat([Buffer.alloc(4), nonce8])
    const sealed = chachaSeal(this.opts.key, nonce12, body, aad)
    sock.send(Buffer.concat([header, sealed, nonce8]), this.opts.port, this.opts.host)
    this._seq = (this._seq + 1) & 0xffff
    this._ts = (this._ts + this._samplesPerFrame) >>> 0
    this._nonce += 1n
  }

  stop(): void {
    this._enc?.stop()
    this._enc = null
    this._sock?.close()
    this._sock = null
    this._pcm = Buffer.alloc(0)
  }
}
