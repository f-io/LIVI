/**
 * bplist — minimal Apple binary property list (bplist00) codec.
 *
 * CarPlay's RTSP-style control channel carries its payloads as bplist00
 * dictionaries. This encodes/decodes just the subset CarPlay uses: dicts,
 * arrays, UTF-8/UTF-16 strings, raw bytes, integers, reals and booleans.
 */

export type PlistValue =
  | boolean
  | number
  | bigint
  | string
  | Buffer
  | PlistValue[]
  | { [key: string]: PlistValue }

const MAGIC = Buffer.from('bplist00', 'ascii')

function writeBE(target: Buffer, offset: number, value: number, size: number): void {
  let v = value
  for (let i = size - 1; i >= 0; i--) {
    target.writeUInt8(v & 0xff, offset + i)
    v = Math.floor(v / 256)
  }
}

// ── Decode ────────────────────────────────────────────────────────────────

export function decodeBplist(buf: Buffer): PlistValue {
  if (buf.length < 8 + 32 || !buf.subarray(0, 8).equals(MAGIC)) {
    throw new Error('bplist: bad magic or too short')
  }
  const trailer = buf.subarray(buf.length - 32)
  const offsetSize = trailer.readUInt8(6)
  const refSize = trailer.readUInt8(7)
  const numObjects = Number(trailer.readBigUInt64BE(8))
  const topObject = Number(trailer.readBigUInt64BE(16))
  const offsetTableOffset = Number(trailer.readBigUInt64BE(24))

  const readSized = (base: number, size: number): number => {
    let v = 0
    for (let i = 0; i < size; i++) v = v * 256 + buf.readUInt8(base + i)
    return v
  }

  const offsets: number[] = []
  for (let i = 0; i < numObjects; i++) {
    offsets.push(readSized(offsetTableOffset + i * offsetSize, offsetSize))
  }

  const readObject = (index: number): PlistValue => {
    let p = offsets[index]
    const marker = buf.readUInt8(p)
    const type = marker >> 4
    const nib = marker & 0x0f
    p += 1

    const readCount = (): number => {
      if (nib !== 0x0f) return nib
      const szMarker = buf.readUInt8(p)
      p += 1
      const intBytes = 1 << (szMarker & 0x0f)
      const c = readSized(p, intBytes)
      p += intBytes
      return c
    }

    switch (type) {
      case 0x0:
        if (nib === 0x08) return false
        if (nib === 0x09) return true
        throw new Error(`bplist: unsupported primitive 0x0${nib.toString(16)}`)
      case 0x1: {
        const nbytes = 1 << nib
        let v = 0n
        for (let i = 0; i < nbytes; i++) v = (v << 8n) | BigInt(buf.readUInt8(p + i))
        return v <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(v) : v
      }
      case 0x2: {
        const nbytes = 1 << nib
        if (nbytes === 4) return buf.readFloatBE(p)
        if (nbytes === 8) return buf.readDoubleBE(p)
        throw new Error('bplist: unsupported real size')
      }
      case 0x4: {
        const n = readCount()
        return Buffer.from(buf.subarray(p, p + n))
      }
      case 0x5: {
        const n = readCount()
        return buf.toString('ascii', p, p + n)
      }
      case 0x6: {
        const n = readCount()
        // bplist unicode strings are UTF-16 big-endian; Node reads little-endian.
        return Buffer.from(buf.subarray(p, p + n * 2))
          .swap16()
          .toString('utf16le')
      }
      case 0xa: {
        const n = readCount()
        const arr: PlistValue[] = []
        for (let i = 0; i < n; i++) arr.push(readObject(readSized(p + i * refSize, refSize)))
        return arr
      }
      case 0xd: {
        const n = readCount()
        const obj: { [k: string]: PlistValue } = {}
        for (let i = 0; i < n; i++) {
          const k = readObject(readSized(p + i * refSize, refSize))
          const v = readObject(readSized(p + (n + i) * refSize, refSize))
          obj[String(k)] = v
        }
        return obj
      }
      default:
        throw new Error(`bplist: unsupported object type 0x${type.toString(16)}`)
    }
  }

  return readObject(topObject)
}

// ── Encode ──────────────────────────────────────────────────────────────────

type EncNode = { kind: 'leaf'; body: Buffer } | { kind: 'container'; head: Buffer; refs: number[] }

function isAscii(s: string): boolean {
  for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) > 0x7f) return false
  return true
}

function marker(type: number, count: number): Buffer {
  if (count < 0x0f) return Buffer.from([(type << 4) | count])
  const bytes = count > 0xffff ? 4 : count > 0xff ? 2 : 1
  const log = bytes === 4 ? 2 : bytes === 2 ? 1 : 0
  const intObj = Buffer.alloc(1 + bytes)
  intObj.writeUInt8(0x10 | log, 0)
  writeBE(intObj, 1, count, bytes)
  return Buffer.concat([Buffer.from([(type << 4) | 0x0f]), intObj])
}

function encodeInt(n: bigint): Buffer {
  const bytes = n > 0xffffffffn ? 8 : n > 0xffffn ? 4 : n > 0xffn ? 2 : 1
  const log = bytes === 8 ? 3 : bytes === 4 ? 2 : bytes === 2 ? 1 : 0
  const b = Buffer.alloc(1 + bytes)
  b.writeUInt8(0x10 | log, 0)
  let v = n
  for (let i = bytes; i >= 1; i--) {
    b.writeUInt8(Number(v & 0xffn), i)
    v >>= 8n
  }
  return b
}

export function encodeBplist(root: PlistValue): Buffer {
  const nodes: EncNode[] = []

  const add = (value: PlistValue): number => {
    const idx = nodes.length
    nodes.push({ kind: 'leaf', body: Buffer.alloc(0) }) // reserve slot

    if (typeof value === 'boolean') {
      nodes[idx] = { kind: 'leaf', body: Buffer.from([value ? 0x09 : 0x08]) }
    } else if (typeof value === 'bigint') {
      nodes[idx] = { kind: 'leaf', body: encodeInt(value) }
    } else if (typeof value === 'number') {
      if (Number.isInteger(value) && value >= 0) {
        nodes[idx] = { kind: 'leaf', body: encodeInt(BigInt(value)) }
      } else {
        const b = Buffer.alloc(9)
        b.writeUInt8(0x23, 0)
        b.writeDoubleBE(value, 1)
        nodes[idx] = { kind: 'leaf', body: b }
      }
    } else if (typeof value === 'string') {
      if (isAscii(value)) {
        nodes[idx] = {
          kind: 'leaf',
          body: Buffer.concat([marker(0x5, value.length), Buffer.from(value, 'ascii')])
        }
      } else {
        const u = Buffer.from(value, 'utf16le').swap16()
        nodes[idx] = { kind: 'leaf', body: Buffer.concat([marker(0x6, value.length), u]) }
      }
    } else if (Buffer.isBuffer(value)) {
      nodes[idx] = { kind: 'leaf', body: Buffer.concat([marker(0x4, value.length), value]) }
    } else if (Array.isArray(value)) {
      const refs = value.map((v) => add(v))
      nodes[idx] = { kind: 'container', head: marker(0xa, refs.length), refs }
    } else {
      const keys = Object.keys(value)
      const keyRefs = keys.map((k) => add(k))
      const valRefs = keys.map((k) => add(value[k]))
      nodes[idx] = {
        kind: 'container',
        head: marker(0xd, keys.length),
        refs: [...keyRefs, ...valRefs]
      }
    }
    return idx
  }

  const topIndex = add(root)
  const numObjects = nodes.length
  const refSize = numObjects > 0xffff ? 4 : numObjects > 0xff ? 2 : 1

  const serialized = nodes.map((node) => {
    if (node.kind === 'leaf') return node.body
    const refBuf = Buffer.alloc(node.refs.length * refSize)
    node.refs.forEach((r, i) => writeBE(refBuf, i * refSize, r, refSize))
    return Buffer.concat([node.head, refBuf])
  })

  const parts: Buffer[] = [MAGIC]
  const offsets: number[] = []
  let cursor = MAGIC.length
  for (const s of serialized) {
    offsets.push(cursor)
    parts.push(s)
    cursor += s.length
  }

  const offsetTableOffset = cursor
  const offsetSize = cursor > 0xffff ? 4 : cursor > 0xff ? 2 : 1
  for (const off of offsets) {
    const b = Buffer.alloc(offsetSize)
    writeBE(b, 0, off, offsetSize)
    parts.push(b)
  }

  const trailer = Buffer.alloc(32)
  trailer.writeUInt8(offsetSize, 6)
  trailer.writeUInt8(refSize, 7)
  trailer.writeBigUInt64BE(BigInt(numObjects), 8)
  trailer.writeBigUInt64BE(BigInt(topIndex), 16)
  trailer.writeBigUInt64BE(BigInt(offsetTableOffset), 24)
  parts.push(trailer)

  return Buffer.concat(parts)
}
