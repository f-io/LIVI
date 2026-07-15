/**
 * screenStream — receives the CarPlay main-screen video on its data port.
 *
 * After SETUP the phone opens a TCP connection to the port we advertised. Each
 * message is a 128-byte AirPlayScreenHeader followed by a body: VideoConfig
 * (avcC/hvcC) or VideoFrame (length-prefixed NALUs), ChaCha20-Poly1305 encrypted
 * with the header as AAD. We decrypt, rewrite to Annex-B, and emit it for the
 * gst-host pipeline. No frame data is decoded here.
 */

import { EventEmitter } from 'node:events'
import net from 'node:net'
import { chachaOpen, nonce64 } from './crypto'
import { avccFrameToAnnexB, configToAnnexB } from './nalu'

const HEADER_LEN = 128
const OP_VIDEO_FRAME = 0
const OP_VIDEO_CONFIG = 1
const MAX_BODY = 8 * 1024 * 1024

export class ScreenStream extends EventEmitter {
  private _server: net.Server | null = null
  private _counter = 0n

  constructor(
    private codec: 'h264' | 'h265',
    private readonly key: Buffer
  ) {
    super()
  }

  /** Listen on an ephemeral port and return it once bound. */
  listen(): Promise<number> {
    return new Promise((resolve, reject) => {
      const server = net.createServer((sock) => this._onConnection(sock))
      server.on('error', reject)
      server.listen({ port: 0, host: '::', ipv6Only: false }, () => {
        const addr = server.address()
        this._server = server
        resolve(typeof addr === 'object' && addr ? addr.port : 0)
      })
    })
  }

  stop(): void {
    this._server?.close()
    this._server = null
  }

  private _onConnection(sock: net.Socket): void {
    console.log(`[cpScreen] video data connection from ${sock.remoteAddress}:${sock.remotePort}`)
    let acc = Buffer.alloc(0)
    sock.on('data', (chunk: Buffer) => {
      acc = Buffer.concat([acc, chunk])
      for (;;) {
        if (acc.length < HEADER_LEN) break
        const bodySize = acc.readUInt32LE(0)
        if (bodySize > MAX_BODY) {
          console.warn(`[cpScreen] implausible bodySize ${bodySize}, dropping connection`)
          sock.destroy()
          return
        }
        if (acc.length < HEADER_LEN + bodySize) break
        const header = acc.subarray(0, HEADER_LEN)
        const body = acc.subarray(HEADER_LEN, HEADER_LEN + bodySize)
        acc = acc.subarray(HEADER_LEN + bodySize)
        try {
          this._onMessage(header, body)
        } catch (e) {
          console.warn('[cpScreen] frame error:', (e as Error).message)
        }
      }
    })
    sock.on('error', (err) => console.warn(`[cpScreen] socket error: ${err.message}`))
    sock.on('close', () => console.log('[cpScreen] video data connection closed'))
  }

  private _onMessage(header: Buffer, body: Buffer): void {
    const opcode = header[4]

    if (opcode === OP_VIDEO_CONFIG) {
      // VideoConfig is sent in the clear (the avcC/hvcC atom); detect the real
      // codec from it (the phone may pick H.264 even when H.265 is offered).
      const { codec, annexB } = configToAnnexB(body)
      this.codec = codec
      this.emit('codec', codec)
      console.log(`[cpScreen] video config (${codec}, ${annexB.length}B annexB)`)
      this.emit('config', annexB)
    } else if (opcode === OP_VIDEO_FRAME) {
      // VideoFrame is ChaCha20-Poly1305 sealed with the 128-byte header as AAD;
      // the nonce is an 8-byte LE counter that advances only on decoded frames.
      let payload = body
      if (body.length >= 16) {
        payload = chachaOpen(this.key, nonce64(this._counter), body, header)
        this._counter++
      }
      const annexB = avccFrameToAnnexB(payload)
      this.emit('frame', annexB)
    }
    // KeepAlive / ForceKeyFrame / Ignore carry no displayable payload.
  }
}
