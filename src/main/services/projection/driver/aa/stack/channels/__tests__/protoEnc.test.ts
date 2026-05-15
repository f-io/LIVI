import {
  decodeFields,
  decodeStart,
  decodeVarintValue,
  encodeVarint,
  fieldFloat,
  fieldLenDelim,
  fieldVarint,
  readVarint,
  tag
} from '../protoEnc'

describe('encodeVarint', () => {
  test.each([
    [0, [0x00]],
    [1, [0x01]],
    [127, [0x7f]],
    [128, [0x80, 0x01]],
    [300, [0xac, 0x02]],
    [0xffffffff, [0xff, 0xff, 0xff, 0xff, 0x0f]]
  ])('encodes %s correctly', (value, expected) => {
    expect(Array.from(encodeVarint(value))).toEqual(expected)
  })

  test('encodes a negative int32 as a 10-byte uint64 two’s-complement varint', () => {
    const buf = encodeVarint(-1)
    expect(buf.length).toBe(10)
    // -1 → 0xFFFFFFFFFFFFFFFF → 10×0x7f varints, top bit set on first 9
    expect(buf[buf.length - 1]).toBe(0x01)
  })

  test('accepts bigint inputs', () => {
    const buf = encodeVarint(1n << 50n)
    expect(buf.length).toBeGreaterThan(5)
  })
})

describe('tag', () => {
  test('packs fieldNumber+wireType correctly', () => {
    // field=1, wire=0 → 0x08
    expect(tag(1, 0)[0]).toBe(0x08)
    // field=2, wire=2 → 0x12
    expect(tag(2, 2)[0]).toBe(0x12)
    // field=15, wire=5 → 0x7d
    expect(tag(15, 5)[0]).toBe(0x7d)
  })
})

describe('fieldVarint', () => {
  test('emits <tag, varint>', () => {
    const buf = fieldVarint(1, 300)
    // tag(1,0)=0x08, varint(300)=0xac,0x02
    expect(Array.from(buf)).toEqual([0x08, 0xac, 0x02])
  })
})

describe('fieldLenDelim', () => {
  test('emits <tag, len, bytes>', () => {
    const data = Buffer.from('abc')
    const buf = fieldLenDelim(2, data)
    expect(buf[0]).toBe(0x12) // tag(2,2)
    expect(buf[1]).toBe(3) // length
    expect(buf.subarray(2).toString()).toBe('abc')
  })
})

describe('fieldFloat', () => {
  test('emits <tag, 4-byte LE float>', () => {
    const buf = fieldFloat(3, 1.5)
    expect(buf[0]).toBe(0x1d) // tag(3,5)
    expect(buf.length).toBe(5)
    expect(buf.readFloatLE(1)).toBeCloseTo(1.5)
  })
})

describe('readVarint', () => {
  test('reads a single-byte varint and reports its length', () => {
    const [v, n] = readVarint(Buffer.from([0x05]), 0)
    expect(v).toBe(5)
    expect(n).toBe(1)
  })

  test('reads a multi-byte varint', () => {
    const [v, n] = readVarint(Buffer.from([0xac, 0x02]), 0)
    expect(v).toBe(300)
    expect(n).toBe(2)
  })

  test('honours offset', () => {
    const [v, n] = readVarint(Buffer.from([0xaa, 0xac, 0x02]), 1)
    expect(v).toBe(300)
    expect(n).toBe(2)
  })

  test('protects against varint overflow (more than 5 bytes)', () => {
    const [, n] = readVarint(Buffer.from([0x80, 0x80, 0x80, 0x80, 0x80, 0x01]), 0)
    expect(n).toBeGreaterThan(0)
  })
})

describe('decodeStart', () => {
  test('decodes session_id (field 1) and configuration_index (field 2)', () => {
    // field 1 = 7, field 2 = 3
    const payload = Buffer.concat([fieldVarint(1, 7).subarray(0), fieldVarint(2, 3).subarray(0)])
    expect(decodeStart(payload)).toEqual({ sessionId: 7, configIndex: 3 })
  })

  test('returns null when session_id is missing', () => {
    const payload = fieldVarint(2, 3)
    expect(decodeStart(payload)).toBeNull()
  })

  test('skips unknown fields', () => {
    const payload = Buffer.concat([
      Buffer.from([0x18]), // unknown field 3 varint
      Buffer.from([0x05]),
      fieldVarint(1, 7)
    ])
    expect(decodeStart(payload)).toEqual({ sessionId: 7, configIndex: -1 })
  })
})

describe('decodeFields', () => {
  test('yields varint fields with their wire type', () => {
    const payload = fieldVarint(1, 42)
    const out = Array.from(decodeFields(payload))
    expect(out).toHaveLength(1)
    expect(out[0].field).toBe(1)
    expect(out[0].wire).toBe(0)
    expect(decodeVarintValue(out[0].bytes)).toBe(42)
  })

  test('yields length-delimited fields with the inner bytes', () => {
    const payload = fieldLenDelim(2, Buffer.from('xyz'))
    const out = Array.from(decodeFields(payload))
    expect(out[0].wire).toBe(2)
    expect(out[0].bytes.toString()).toBe('xyz')
  })

  test('yields fixed32 (float) fields', () => {
    const payload = fieldFloat(3, 2.25)
    const out = Array.from(decodeFields(payload))
    expect(out[0].wire).toBe(5)
    expect(out[0].bytes.readFloatLE(0)).toBe(2.25)
  })

  test('walks multiple fields', () => {
    const payload = Buffer.concat([
      fieldVarint(1, 10),
      fieldLenDelim(2, Buffer.from('hi')),
      fieldFloat(3, 1.5)
    ])
    const fields = Array.from(decodeFields(payload))
    expect(fields.map((f) => f.field)).toEqual([1, 2, 3])
  })

  test('yields fixed64 fields', () => {
    // field=4, wire=1 → tag 0x21, plus 8 bytes
    const payload = Buffer.concat([Buffer.from([0x21]), Buffer.alloc(8, 0xff)])
    const fields = Array.from(decodeFields(payload))
    expect(fields[0].field).toBe(4)
    expect(fields[0].wire).toBe(1)
    expect(fields[0].bytes.length).toBe(8)
  })

  test('stops on an unknown wire type (groups)', () => {
    const payload = Buffer.from([0x03 /* wire=3 (start group) */])
    const fields = Array.from(decodeFields(payload))
    expect(fields).toHaveLength(0)
  })
})
