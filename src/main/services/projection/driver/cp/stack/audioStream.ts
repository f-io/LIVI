/**
 * audioStream — receives one CarPlay audio stream on its UDP data port.
 *
 * After SETUP the phone sends RTP/UDP packets to the port we advertised. Each
 * packet is [12B RTP header][ciphertext][8B nonce LE][16B auth tag], sealed with
 * ChaCha20-Poly1305 using the RTP timestamp+SSRC as AAD and the per-stream
 * DataStream output key. We decrypt each payload and emit it on 'pcm'; for a
 * compressed stream (AAC-LC/Opus) that payload is still an encoded access unit
 * that CpAudioDecoder turns into PCM, for a raw stream it is PCM already. The
 * stream latches 'active' true on its first packet and false on stop(); this
 * brackets the LIVI audio output start/stop exactly once. We must NOT
 * toggle on silence: buffered media arrives in bursts with gaps,
 * and a per-gap AudioMediaStop would flap mediaActive and re-trigger the sink
 * warmup mute, killing playback, volume and the FFT tap.
 */

import dgram from 'node:dgram'
import { EventEmitter } from 'node:events'
import { chachaOpen } from './crypto'

const RTP_HEADER_LEN = 12
const TAIL_LEN = 24 // 8-byte little-endian nonce + 16-byte auth tag
const NTP_EPOCH_OFFSET = 2208988800 // seconds between 1900-01-01 and 1970-01-01

/** Current time as a 64-bit NTP timestamp, matching TimingSync's clock (Date.now). */
export function ntp64Now(): bigint {
  const t = Date.now() / 1000 + NTP_EPOCH_OFFSET
  const sec = Math.floor(t)
  const frac = Math.floor((t - sec) * 0x100000000)
  return (BigInt(sec) << 32n) | BigInt(frac >>> 0)
}

/**
 * Media-clock origin captured at the stream's first packet.
 */
export interface MediaClockOrigin {
  firstSample: number
  originNs: bigint
}

export class AudioStream extends EventEmitter {
  private _data: dgram.Socket | null = null
  private _control: dgram.Socket | null = null
  private _active = false
  private _origin: MediaClockOrigin | null = null
  private _lastRecvSample = 0

  constructor(
    private readonly key: Buffer,
    private readonly label: string
  ) {
    super()
  }

  /** Media-clock origin (first packet's RTP sample + time), or null before any packet. */
  getOrigin(): MediaClockOrigin | null {
    return this._origin
  }

  /** Latest received RTP sample position (diagnostics: buffer = recv - played). */
  getLastRecvSample(): number {
    return this._lastRecvSample
  }

  /** Bind the UDP data + control ports and return them for the SETUP response. */
  async listen(): Promise<{ dataPort: number; controlPort: number }> {
    const dataPort = await this._bind((sock) => {
      this._data = sock
      sock.on('message', (pkt) => this._onPacket(pkt))
    })
    const controlPort = await this._bind((sock) => {
      this._control = sock
      // RTCP (timing + retransmit requests). We don't drive it yet; keep the port
      // bound and drain so the phone's control path has a live peer.
      sock.on('message', () => {})
    })
    return { dataPort, controlPort }
  }

  stop(): void {
    if (this._active) {
      this._active = false
      this.emit('active', false)
    }
    this._data?.close()
    this._control?.close()
    this._data = null
    this._control = null
  }

  private _bind(setup: (sock: dgram.Socket) => void): Promise<number> {
    return new Promise((resolve, reject) => {
      const sock = dgram.createSocket({ type: 'udp6', ipv6Only: false })
      sock.on('error', reject)
      setup(sock)
      sock.bind(0, '::', () => resolve(sock.address().port))
    })
  }

  private _onPacket(pkt: Buffer): void {
    if (pkt.length < RTP_HEADER_LEN + TAIL_LEN) return
    const len = pkt.length
    // AAD is the RTP timestamp + SSRC (the last 8 bytes of the 12-byte RTP header),
    // not the whole header. Tail layout: [ciphertext][16-byte tag][8-byte nonce LE].
    const aad = pkt.subarray(4, RTP_HEADER_LEN)
    const nonce8 = pkt.subarray(len - 8, len)
    const tag = pkt.subarray(len - 24, len - 8)
    const ct = pkt.subarray(RTP_HEADER_LEN, len - 24)
    const nonce = Buffer.concat([Buffer.alloc(4), nonce8])
    let pcm: Buffer
    try {
      pcm = chachaOpen(this.key, nonce, Buffer.concat([ct, tag]), aad)
    } catch (e) {
      console.warn(`[cpAudio:${this.label}] decrypt failed: ${(e as Error).message}`)
      return
    }
    const sample = pkt.readUInt32BE(4)
    this._lastRecvSample = sample
    if (!this._active) {
      // Latch active on the first packet: brackets the LIVI output start once, and
      // pin the media-clock origin here (RTP sample position + monotonic time).
      this._active = true
      this.emit('active', true)
      this._origin = { firstSample: sample, originNs: process.hrtime.bigint() }
    }
    this.emit('pcm', pcm)
    // OPUS streams are decoded from RTP (rtpjitterbuffer/rtpopusdepay), so also offer
    // the reconstructed RTP packet (original header + decrypted payload). Only built
    // when someone listens.
    if (this.listenerCount('rtp') > 0) {
      this.emit('rtp', Buffer.concat([pkt.subarray(0, RTP_HEADER_LEN), pcm]))
    }
  }
}
