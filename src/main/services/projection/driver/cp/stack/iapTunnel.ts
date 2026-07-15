/**
 * iapTunnel — iAP2-over-CarPlay DataStream transport (stream type 130, iAP UUID).
 *
 * After the phone sends disableBluetooth, iAP2 must continue over a dedicated TCP
 * DataStream instead of Bluetooth. Two layers ride the socket:
 *   1. NetSocketChaCha20Poly1305 stream framing: [2B LE len][ciphertext][16B tag],
 *      nonce = an 8-byte little-endian per-frame counter (separate read/write, both
 *      from 0), AAD = the 2-byte length prefix, <=16 KB plaintext per frame. Keys
 *      derive from the SETUP seed exactly like the screen/audio DataStreams.
 *   2. APTransportPackage framing on the decrypted byte stream: a 32-byte big-endian
 *      header (size, packageType, groupID, messageType, replyToken, replyErr) then
 *      the body. For iAP2 the messageType is 'cmnd' and the body is raw iAP2 bytes.
 *
 * We emit the raw iAP2 bytes ('iap') for CpStack to relay to the Python iAP2 stack.
 * The reverse direction (accessory -> phone) rides the AirPlay event channel, not this
 * socket, so this transport is receive-only.
 */

import { EventEmitter } from 'node:events'
import net from 'node:net'
import { chachaOpen, hkdfSha512, nonce64 } from './crypto'

const PKG_HEADER = 32
const MSG_TYPE_COMM = 0x636f6d6d // 'comm' — the iAP DataStream message type
const MAX_PACKAGE = 4 * 1024 * 1024

export class IapTunnel extends EventEmitter {
  private _server: net.Server | null = null
  private _sock: net.Socket | null = null
  private readonly _readKey: Buffer
  private _readCtr = 0n
  private _cipherBuf = Buffer.alloc(0)
  private _plainBuf = Buffer.alloc(0)

  constructor(shared: Buffer, seed: bigint | number) {
    super()
    // Salt is "DataStream-Salt" + the seed as decimal (PRIu64). Read (phone -> us)
    // uses the Output key, like the screen path.
    const salt = `DataStream-Salt${seed}`
    this._readKey = hkdfSha512(shared, salt, 'DataStream-Output-Encryption-Key', 32)
  }

  /** Listen on an ephemeral TCP port for the phone's iAP data connection. */
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
    this._sock?.destroy()
    this._server?.close()
    this._sock = null
    this._server = null
  }

  private _onConnection(sock: net.Socket): void {
    console.log(`[cpIapTunnel] iAP data connection from ${sock.remoteAddress}:${sock.remotePort}`)
    this._sock = sock
    this._cipherBuf = Buffer.alloc(0)
    this._plainBuf = Buffer.alloc(0)
    this._readCtr = 0n
    sock.on('data', (chunk: Buffer) => this._onData(chunk))
    sock.on('error', (e) => console.warn(`[cpIapTunnel] socket error: ${e.message}`))
    sock.on('close', () => {
      if (this._sock === sock) this._sock = null
      console.log('[cpIapTunnel] iAP data connection closed')
      this.emit('closed')
    })
    this.emit('open')
  }

  private _onData(chunk: Buffer): void {
    this._cipherBuf = Buffer.concat([this._cipherBuf, chunk])
    // Layer 1: decrypt each complete stream frame into the plaintext buffer.
    for (;;) {
      if (this._cipherBuf.length < 2) break
      const len = this._cipherBuf.readUInt16LE(0)
      const frameLen = 2 + len + 16
      if (this._cipherBuf.length < frameLen) break
      const aad = this._cipherBuf.subarray(0, 2)
      const sealed = this._cipherBuf.subarray(2, frameLen) // ciphertext || tag
      this._cipherBuf = this._cipherBuf.subarray(frameLen)
      try {
        const plain = chachaOpen(this._readKey, nonce64(this._readCtr), sealed, aad)
        this._readCtr++
        this._plainBuf = Buffer.concat([this._plainBuf, plain])
      } catch (e) {
        console.warn(`[cpIapTunnel] stream decrypt failed: ${(e as Error).message}`)
        this._sock?.destroy()
        return
      }
    }
    // Layer 2: parse APTransportPackages and relay the iAP2 bodies.
    for (;;) {
      if (this._plainBuf.length < PKG_HEADER) break
      const size = this._plainBuf.readUInt32BE(0)
      if (size < PKG_HEADER || size > MAX_PACKAGE) {
        console.warn(`[cpIapTunnel] implausible package size ${size}, dropping`)
        this._sock?.destroy()
        return
      }
      if (this._plainBuf.length < size) break
      const messageType = this._plainBuf.readUInt32BE(16)
      const body = Buffer.from(this._plainBuf.subarray(PKG_HEADER, size))
      this._plainBuf = this._plainBuf.subarray(size)
      if (messageType === MSG_TYPE_COMM) this.emit('iap', body)
    }
  }
}
