/**
 * controlCipher — ChaCha20-Poly1305 framing for the CarPlay control channel.
 *
 * After pair-verify, every RTSP message on TCP :7000 travels as one or more
 * frames: [2B length LE (ciphertext only)][ciphertext][16B tag]. The 2-byte
 * length header is the AEAD associated data. The nonce is a per-direction
 * 8-byte little-endian counter that increments after each frame. Read and write
 * use separate keys.
 */

import { chachaOpen, chachaSeal, nonce64 } from './crypto'

const MAX_PAYLOAD = 0x4000 // 16384

export class ControlCipher {
  private readCtr = 0n
  private writeCtr = 0n

  constructor(
    private readonly readKey: Buffer,
    private readonly writeKey: Buffer
  ) {}

  /** Decrypt as many whole frames as `buf` holds; return plaintext + leftover. */
  decrypt(buf: Buffer): { data: Buffer; rest: Buffer } {
    const out: Buffer[] = []
    let off = 0
    while (buf.length - off >= 2) {
      const len = buf.readUInt16LE(off)
      const frameEnd = off + 2 + len + 16
      if (buf.length < frameEnd) break
      const aad = buf.subarray(off, off + 2)
      const ctAndTag = buf.subarray(off + 2, frameEnd)
      out.push(chachaOpen(this.readKey, nonce64(this.readCtr), ctAndTag, aad))
      this.readCtr++
      off = frameEnd
    }
    return { data: Buffer.concat(out), rest: buf.subarray(off) }
  }

  /** Frame and encrypt one plaintext message, splitting at the 16 KiB limit. */
  encrypt(plain: Buffer): Buffer {
    const out: Buffer[] = []
    let i = 0
    do {
      const chunk = plain.subarray(i, i + MAX_PAYLOAD)
      const header = Buffer.alloc(2)
      header.writeUInt16LE(chunk.length, 0)
      out.push(header, chachaSeal(this.writeKey, nonce64(this.writeCtr), chunk, header))
      this.writeCtr++
      i += MAX_PAYLOAD
    } while (i < plain.length)
    return Buffer.concat(out)
  }
}
