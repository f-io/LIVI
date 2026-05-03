/**
 * Tiny protobuf wire-format encoders.
 *
 * Used by Audio/Video/InputChannel for the few small messages we need to send
 * per frame (Ack, InputReport, …).
 *
 * Wire-type values (proto2/proto3 are identical here):
 *   0 = varint           (int32, int64, uint32, uint64, bool, enum)
 *   1 = fixed64          (fixed64, sfixed64, double)
 *   2 = length-delimited (string, bytes, sub-message, packed repeated)
 *   5 = fixed32          (fixed32, sfixed32, float)
 */

/** Encode a base-128 varint. Accepts number or bigint; bigint is required for full uint64 range. */
export function encodeVarint(value: number | bigint): Buffer {
  let v = typeof value === 'bigint' ? value : BigInt(value)
  // Two's-complement wrap for negative int32/int64 inputs (proto signs them as
  // varints of their unsigned 64-bit bit pattern).
  if (v < 0n) v = (v + (1n << 64n)) & ((1n << 64n) - 1n)
  const bytes: number[] = []
  while (v > 0x7fn) {
    bytes.push(Number((v & 0x7fn) | 0x80n))
    v >>= 7n
  }
  bytes.push(Number(v & 0x7fn))
  return Buffer.from(bytes)
}

/** Encode a `(fieldNumber, wireType)` tag as varint. */
export function tag(fieldNumber: number, wireType: number): Buffer {
  return encodeVarint((fieldNumber << 3) | wireType)
}

/** Emit `<tag, varint>` for a varint-typed field. */
export function fieldVarint(fieldNumber: number, value: number | bigint): Buffer {
  return Buffer.concat([tag(fieldNumber, 0), encodeVarint(value)])
}

/** Emit `<tag, len, bytes>` for a length-delimited field. */
export function fieldLenDelim(fieldNumber: number, data: Buffer): Buffer {
  return Buffer.concat([tag(fieldNumber, 2), encodeVarint(data.length), data])
}

/** Emit `<tag, 4-byte LE float>` for a fixed32 (float) field. */
export function fieldFloat(fieldNumber: number, value: number): Buffer {
  const buf = Buffer.alloc(4)
  buf.writeFloatLE(value, 0)
  return Buffer.concat([tag(fieldNumber, 5), buf])
}

/**
 * Decode a varint at `off`, returning `[value, bytesRead]`. Reads up to 10
 * bytes. Use BigInt return when callers need the full uint64 range; here we
 * stay within int32-safe values for session_id / configuration_index.
 */
export function readVarint(buf: Buffer, off: number): [number, number] {
  let result = 0
  let shift = 0
  let pos = off
  while (pos < buf.length) {
    const b = buf[pos]!
    result |= (b & 0x7f) << shift
    pos += 1
    if ((b & 0x80) === 0) return [result >>> 0, pos - off]
    shift += 7
    if (shift >= 32) {
      // overflow protection
      while (pos < buf.length && (buf[pos]! & 0x80) !== 0) pos += 1
      pos += 1
      return [result >>> 0, pos - off]
    }
  }
  return [result >>> 0, pos - off]
}

/**
 * Decode the `Start { session_id=1, configuration_index=2 }` proto from the
 * payload of an AV_MSG.START_INDICATION. Returns `null` for malformed input.
 *
 * Wire format: field-1 tag=0x08 varint, field-2 tag=0x10 varint.
 */
export function decodeStart(payload: Buffer): { sessionId: number; configIndex: number } | null {
  let off = 0
  let sessionId = -1
  let configIndex = -1
  while (off < payload.length) {
    const t = payload[off]!
    off += 1
    if (t === 0x08) {
      const [v, n] = readVarint(payload, off)
      sessionId = v
      off += n
    } else if (t === 0x10) {
      const [v, n] = readVarint(payload, off)
      configIndex = v
      off += n
    } else {
      // unknown field — skip varint to keep walking
      const [, n] = readVarint(payload, off)
      off += n
    }
  }
  if (sessionId < 0) return null
  return { sessionId, configIndex }
}

/**
 * Generic proto decoder — walks every field in `payload` and yields
 * `(fieldNumber, wireType, valueBytes)` tuples for the caller to dispatch.
 */
export function* decodeFields(
  payload: Buffer
): Generator<{ field: number; wire: number; bytes: Buffer }> {
  let off = 0
  while (off < payload.length) {
    const [tag, tn] = readVarint(payload, off)
    off += tn
    const wire = tag & 0x7
    const field = tag >>> 3
    if (wire === 0) {
      const [, vn] = readVarint(payload, off)
      yield { field, wire, bytes: payload.subarray(off, off + vn) }
      off += vn
    } else if (wire === 1) {
      yield { field, wire, bytes: payload.subarray(off, off + 8) }
      off += 8
    } else if (wire === 2) {
      const [len, ln] = readVarint(payload, off)
      off += ln
      yield { field, wire, bytes: payload.subarray(off, off + len) }
      off += len
    } else if (wire === 5) {
      yield { field, wire, bytes: payload.subarray(off, off + 4) }
      off += 4
    } else {
      // groups / unknown
      return
    }
  }
}

/** Decode a varint-encoded field value (consumes the entire bytes buffer). */
export function decodeVarintValue(bytes: Buffer): number {
  return readVarint(bytes, 0)[0]
}
