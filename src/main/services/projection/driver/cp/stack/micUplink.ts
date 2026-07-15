import dgram from 'node:dgram'
import { chachaSeal } from './crypto'

const RTP_HEADER_LEN = 12
const FRAME_MS = 20

export class CpMicUplink {
  private _sock: dgram.Socket | null = null
  private _seq = 0
  private _ts = 0
  private _nonce = 0n
  private _pcm = Buffer.alloc(0)
  private readonly _ssrc: number
  private readonly _samplesPerFrame: number
  private readonly _frameBytes: number

  constructor(
    private readonly key: Buffer,
    private readonly host: string,
    private readonly port: number,
    private readonly sampleRate: number,
    private readonly channels: number,
    private readonly payloadType: number,
    ssrc: number
  ) {
    this._ssrc = ssrc >>> 0
    this._samplesPerFrame = Math.round((sampleRate * FRAME_MS) / 1000)
    this._frameBytes = this._samplesPerFrame * channels * 2
  }

  start(): void {
    if (this._sock) return
    this._sock = dgram.createSocket({ type: 'udp6', ipv6Only: false })
    this._sock.on('error', () => {})
    this._sock.bind(0, '::')
  }

  write(pcmLe: Buffer): void {
    if (!this._sock) return
    this._pcm = Buffer.concat([this._pcm, pcmLe])
    while (this._pcm.length >= this._frameBytes) {
      const frame = Buffer.from(this._pcm.subarray(0, this._frameBytes))
      this._pcm = this._pcm.subarray(this._frameBytes)
      this._send(frame)
    }
  }

  private _send(pcmLe: Buffer): void {
    const sock = this._sock
    if (!sock) return
    pcmLe.swap16()
    const header = Buffer.allocUnsafe(RTP_HEADER_LEN)
    header[0] = 0x80
    header[1] = this.payloadType & 0x7f
    header.writeUInt16BE(this._seq & 0xffff, 2)
    header.writeUInt32BE(this._ts >>> 0, 4)
    header.writeUInt32BE(this._ssrc, 8)
    const aad = header.subarray(4, RTP_HEADER_LEN)
    const nonce8 = Buffer.allocUnsafe(8)
    nonce8.writeBigUInt64LE(this._nonce)
    const nonce12 = Buffer.concat([Buffer.alloc(4), nonce8])
    const sealed = chachaSeal(this.key, nonce12, pcmLe, aad)
    sock.send(Buffer.concat([header, sealed, nonce8]), this.port, this.host)
    this._seq = (this._seq + 1) & 0xffff
    this._ts = (this._ts + this._samplesPerFrame) >>> 0
    this._nonce += 1n
  }

  stop(): void {
    this._sock?.close()
    this._sock = null
    this._pcm = Buffer.alloc(0)
  }
}
