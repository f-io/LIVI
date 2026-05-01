/**
 * AA wire-protocol frame codec.
 *
 * aasdk frame layout (channel + flags header is always plaintext, payload may
 * be encrypted). The size header has TWO forms depending on the frame type:
 *
 *   SHORT  (BULK, MIDDLE, LAST):
 *     Byte 0:    channelId (uint8)
 *     Byte 1:    flags     (uint8)
 *     Bytes 2-3: payloadSize (uint16 BE)
 *     Bytes 4..: payload (payloadSize bytes)
 *     → total wire size = 4 + payloadSize
 *
 *   EXTENDED (FIRST only — first fragment of a multi-frame message):
 *     Byte 0:    channelId
 *     Byte 1:    flags          (bit0=FIRST=1, bit1=LAST=0)
 *     Bytes 2-3: payloadSize    (uint16 BE)  ← just THIS fragment's payload
 *     Bytes 4-7: totalSize      (uint32 BE)  ← full reassembled message size
 *     Bytes 8..: payload (payloadSize bytes)
 *     → total wire size = 8 + payloadSize
 *
 * ── Payload format (after size headers are consumed) ──
 *   For pre-TLS / plaintext frames the payload is:
 *     Bytes 0-1: messageId (uint16 BE)
 *     Bytes 2..: protobuf data (or raw bytes for VERSION/SSL_HANDSHAKE)
 *   For encrypted post-TLS frames the payload IS one or more TLS records.
 *   Multi-frame TLS payloads must be reassembled into a single byte stream
 *   before being injected into Node's TLSSocket
 */

export const FRAME_HEADER_SHORT = 4 // ch + flags + payloadSize(2)
export const FRAME_HEADER_EXTENDED = 8 // ch + flags + payloadSize(2) + totalSize(4)
export const FRAME_HEADER_SIZE = FRAME_HEADER_SHORT // back-compat alias

export interface RawFrame {
  channelId: number
  flags: number
  msgId: number // first 2 bytes of payload (after framing)
  payload: Buffer // bytes AFTER the 2-byte msgId (the actual proto data)
  rawPayload: Buffer // full payload including msgId bytes
}

/**
 * Encode a complete BULK frame ready to write to the TCP socket.
 */
export function encodeFrame(channelId: number, flags: number, msgId: number, data: Buffer): Buffer {
  const msgIdBuf = Buffer.allocUnsafe(2)
  msgIdBuf.writeUInt16BE(msgId, 0)
  const fullPayload = Buffer.concat([msgIdBuf, data])

  const header = Buffer.allocUnsafe(FRAME_HEADER_SHORT)
  header.writeUInt8(channelId, 0)
  header.writeUInt8(flags, 1)
  header.writeUInt16BE(fullPayload.length, 2)

  return Buffer.concat([header, fullPayload])
}

/**
 * Streaming frame parser.
 * Feed raw TCP bytes via push(); register callback via onFrame().
 * Handles fragmented TCP reads and multi-frame reassembly.
 *
 * Multi-frame messages (FIRST → MIDDLE* → LAST) are reassembled into a single
 * payload before the onFrame callback fires. The FIRST fragment's EXTENDED
 * size header announces totalSize (full reassembled message size)
 */
export class FrameParser {
  private _buf = Buffer.allocUnsafe(0)

  // Per-channel reassembly state. The announced totalSize comes from the
  // EXTENDED size header on the FIRST fragment.
  private _fragments = new Map<number, { parts: Buffer[]; totalSize: number }>()

  private _onFrame: ((frame: RawFrame) => void) | null = null

  onFrame(cb: (frame: RawFrame) => void): void {
    this._onFrame = cb
  }

  push(chunk: Buffer): void {
    this._buf = Buffer.concat([this._buf, chunk])
    this._drain()
  }

  private _drain(): void {
    while (this._buf.length >= FRAME_HEADER_SHORT) {
      const channelId = this._buf.readUInt8(0)
      const flags = this._buf.readUInt8(1)
      const isFirst = (flags & 0x01) !== 0
      const isLast = (flags & 0x02) !== 0
      const isExtended = isFirst && !isLast
      const headerLen = isExtended ? FRAME_HEADER_EXTENDED : FRAME_HEADER_SHORT

      if (this._buf.length < headerLen) break

      const payloadSize = this._buf.readUInt16BE(2)
      const totalFrame = headerLen + payloadSize
      if (this._buf.length < totalFrame) break

      const announcedTotalSize = isExtended ? this._buf.readUInt32BE(4) : 0
      const rawPayload = Buffer.from(this._buf.subarray(headerLen, totalFrame))

      this._buf = this._buf.subarray(totalFrame)

      this._handleFrame(channelId, flags, rawPayload, announcedTotalSize)
    }
  }

  private _handleFrame(
    channelId: number,
    flags: number,
    payload: Buffer,
    announcedTotalSize: number
  ): void {
    const isFirst = (flags & 0x01) !== 0
    const isLast = (flags & 0x02) !== 0

    // BULK — single-frame message, emit immediately.
    if (isFirst && isLast) {
      this._emit(channelId, flags, payload)
      return
    }

    // FIRST — start reassembly; totalSize was already extracted from the
    // EXTENDED size header by _drain
    if (isFirst && !isLast) {
      this._fragments.set(channelId, { parts: [payload], totalSize: announcedTotalSize })
      return
    }

    // MIDDLE / LAST — append.
    const state = this._fragments.get(channelId)
    if (!state) {
      console.warn(
        `[FrameParser] ch=${channelId} got continuation but no first fragment — dropping`
      )
      return
    }
    state.parts.push(payload)

    if (isLast) {
      this._fragments.delete(channelId)
      const full = Buffer.concat(state.parts)
      if (full.length !== state.totalSize) {
        console.warn(
          `[FrameParser] ch=${channelId} reassembly size mismatch: got ${full.length}B, expected ${state.totalSize}B`
        )
      }
      this._emit(channelId, flags, full)
    }
  }

  private _emit(channelId: number, flags: number, rawPayload: Buffer): void {
    if (!this._onFrame) return
    if (rawPayload.length < 2) {
      console.warn(`[FrameParser] ch=${channelId} payload too short (${rawPayload.length} bytes)`)
      return
    }

    const msgId = rawPayload.readUInt16BE(0)
    const payload = rawPayload.subarray(2)

    this._onFrame({ channelId, flags, msgId, payload, rawPayload })
  }
}
