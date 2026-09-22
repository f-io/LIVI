/**
 * The session socket the helper opens per phone. It delivers decrypted AA
 * messages and takes cleartext to encrypt and send, so the TCP socket and the
 * TLS engine live in the helper.
 *
 * Wire: [len u32 LE][kind u8][body]
 *   kind 0: [ch][flags][msgId u16 BE][payload]   an AA message, either direction
 *   kind 1: a JSON object                          control, either direction
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'

const KIND_MESSAGE = 0
const KIND_CONTROL = 1

export type HelperSessionControl = { type: string; [key: string]: unknown }

export class HelperSessionLink extends EventEmitter {
  // Events: 'message' (ch, flags, msgId, payload), 'control' (obj), 'close', 'error'
  private _buf: Buffer = Buffer.alloc(0)
  private _closed = false

  constructor(
    private readonly _sock: net.Socket,
    readonly peer: string
  ) {
    super()
    _sock.on('data', (chunk: Buffer) => this._onData(chunk))
    _sock.on('error', (err: Error) => this.emit('error', err))
    _sock.on('close', () => {
      this._closed = true
      this.emit('close')
    })
  }

  static connect(path: string, peer: string): Promise<HelperSessionLink> {
    return new Promise((resolve, reject) => {
      const sock = net.createConnection(path)
      sock.once('connect', () => resolve(new HelperSessionLink(sock, peer)))
      sock.once('error', reject)
    })
  }

  get closed(): boolean {
    return this._closed
  }

  send(ch: number, flags: number, msgId: number, payload: Buffer): void {
    if (this._closed || !this._sock.writable) return
    const head = Buffer.allocUnsafe(9)
    head.writeUInt32LE(5 + payload.length, 0)
    head.writeUInt8(KIND_MESSAGE, 4)
    head.writeUInt8(ch, 5)
    head.writeUInt8(flags, 6)
    head.writeUInt16BE(msgId, 7)
    this._sock.write(Buffer.concat([head, payload]))
  }

  control(msg: HelperSessionControl): void {
    if (this._closed || !this._sock.writable) return
    const body = Buffer.from(JSON.stringify(msg), 'utf8')
    const head = Buffer.allocUnsafe(5)
    head.writeUInt32LE(1 + body.length, 0)
    head.writeUInt8(KIND_CONTROL, 4)
    this._sock.write(Buffer.concat([head, body]))
  }

  /** Closes the write side towards the phone, the read side stays. */
  end(): void {
    this.control({ type: 'end' })
  }

  /** Ends the phone connection and this link. */
  destroy(): void {
    if (this._closed) return
    this.control({ type: 'close' })
    this._sock.end()
  }

  private _onData(chunk: Buffer): void {
    this._buf = this._buf.length ? Buffer.concat([this._buf, chunk]) : chunk
    for (;;) {
      if (this._buf.length < 4) break
      const len = this._buf.readUInt32LE(0)
      if (this._buf.length < 4 + len) break
      const body = this._buf.subarray(4, 4 + len)
      this._buf = this._buf.subarray(4 + len)
      if (len < 1) continue
      const kind = body[0]
      if (kind === KIND_MESSAGE && len >= 5) {
        this.emit('message', body[1], body[2], body.readUInt16BE(3), Buffer.from(body.subarray(5)))
      } else if (kind === KIND_CONTROL) {
        try {
          const obj = JSON.parse(body.subarray(1).toString('utf8'))
          if (obj && typeof obj === 'object' && typeof obj.type === 'string') {
            this.emit('control', obj as HelperSessionControl)
          }
        } catch {
          /* not for us */
        }
      }
    }
  }
}
