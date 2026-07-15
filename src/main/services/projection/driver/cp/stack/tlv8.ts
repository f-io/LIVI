/**
 * tlv8 — HomeKit/AirPlay-style TLV8 codec used by the pairing messages.
 *
 * Each item is [1-byte type][1-byte length][value]. Values longer than 255
 * bytes are split into consecutive items of the same type (fragmentation);
 * on decode, consecutive same-type items are concatenated. Cleanroom.
 */

export interface Tlv8Item {
  type: number
  value: Buffer
}

export function encodeTlv8(items: Tlv8Item[]): Buffer {
  const chunks: Buffer[] = []
  let prevType: number | null = null
  for (const item of items) {
    // A separator (zero-length item) is inserted between two adjacent items of
    // the same type so the decoder does not merge them as one fragmented value.
    if (prevType === item.type) chunks.push(Buffer.from([0xff, 0x00]))
    let off = 0
    do {
      const len = Math.min(255, item.value.length - off)
      chunks.push(Buffer.from([item.type, len]))
      chunks.push(item.value.subarray(off, off + len))
      off += len
    } while (off < item.value.length)
    prevType = item.type
  }
  return Buffer.concat(chunks)
}

/** Decode into a map of type -> merged value (consecutive same-type fragments joined). */
export function decodeTlv8(buf: Buffer): Map<number, Buffer> {
  const out = new Map<number, Buffer>()
  let p = 0
  let lastType: number | null = null
  let lastLen = 0
  while (p + 2 <= buf.length) {
    const type = buf.readUInt8(p)
    const len = buf.readUInt8(p + 1)
    const value = buf.subarray(p + 2, p + 2 + len)
    p += 2 + len
    // Fragmentation continues only when the previous item was a full 255 bytes.
    if (type === lastType && lastLen === 255) {
      out.set(type, Buffer.concat([out.get(type) ?? Buffer.alloc(0), value]))
    } else {
      out.set(type, Buffer.from(value))
    }
    lastType = type
    lastLen = len
  }
  return out
}
