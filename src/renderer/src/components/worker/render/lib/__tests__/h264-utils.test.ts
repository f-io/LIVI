import { Bitstream, NALUStream, RawBitstream, SPS } from '../h264-utils'

describe('RawBitstream', () => {
  test('reads and writes primitive bit values', () => {
    const bs = new RawBitstream(32)

    bs.put_u_1(1)
    bs.put_u(0b01, 2)
    bs.put_u8(0xaa)
    bs.put_u(0b1111, 4)

    bs.seek(0)
    expect(bs.u_1()).toBe(1)
    expect(bs.u_2()).toBe(0b01)
    expect(bs.u_8()).toBe(0xaa)
    expect(bs.u(4)).toBe(0b1111)
  })

  test('encodes and decodes Exp-Golomb values', () => {
    const bs = new RawBitstream(128)
    const ueValues = [0, 1, 5, 31]
    const seValues = [0, -1, 1, -3, 7]

    ueValues.forEach((v) => bs.put_ue_v(v))
    seValues.forEach((v) => bs.put_se_v(v))
    bs.put_complete()

    ueValues.forEach((v) => expect(bs.ue_v()).toBe(v))
    seValues.forEach((v) => expect(bs.se_v()).toBe(v))
  })

  test('supports copyBits on aligned and unaligned pointers', () => {
    const src = new RawBitstream(64)
    src.put_u8(0b11110000)
    src.put_u8(0b00001111)

    const dstAligned = new RawBitstream(64)
    dstAligned.copyBits(src, 0, 16, 0)
    dstAligned.seek(0)
    expect(dstAligned.u_8()).toBe(0b11110000)
    expect(dstAligned.u_8()).toBe(0b00001111)

    const dstUnaligned = new RawBitstream(64)
    dstUnaligned.put_u_1(1)
    dstUnaligned.copyBits(src, 3, 10, undefined)
    dstUnaligned.seek(1)
    expect(dstUnaligned.u(10)).toBe(0b1000000001)
  })

  test('supports seek/peek and throws on overflow', () => {
    const bs = new RawBitstream(8)
    bs.put_u8(0b10101010)
    bs.seek(0)

    expect(bs.peek16).toContain('1010')
    expect(bs.remaining).toBe(8)
    expect(bs.consumed).toBe(0)

    bs.seek(8)
    expect(() => bs.u_1()).toThrow('NALUStream error: bitstream exhausted')
    expect(() => bs.seek(9)).toThrow('cannot seek beyond end')
  })

  test('reads unaligned u_8 branch', () => {
    const bs = new RawBitstream(24)
    bs.put_u(0b111, 3)
    bs.put_u8(0b10101010)
    bs.put_u(0b11, 2)
    bs.seek(3)

    expect(bs.u_8()).toBe(0b10101010)
  })
})

describe('Bitstream', () => {
  test('detects and removes emulation prevention bytes', () => {
    const src = new Uint8Array([0x67, 0x00, 0x00, 0x03, 0x01, 0xaa])
    const bs = new Bitstream(src)

    expect(bs.deemulated).toBe(true)
    expect(Array.from(bs.buffer)).toEqual([0x67, 0x00, 0x00, 0x01, 0xaa])

    const restored = Array.from(bs.stream)
    expect(restored.slice(0, src.length)).toEqual(Array.from(src))
  })

  test('keeps clean streams untouched', () => {
    const src = new Uint8Array([0x67, 0x11, 0x22, 0x33])
    const bs = new Bitstream(src)

    expect(bs.deemulated).toBe(false)
    expect(Array.from(bs.stream)).toEqual(Array.from(src))
  })

  test('copyBits inherits deemulated flag', () => {
    const from = new Bitstream(new Uint8Array([0x00, 0x00, 0x03, 0x01]))
    const to = new Bitstream(32)

    to.copyBits(from, 0, 8, 0)
    expect(to.deemulated).toBe(from.deemulated)
  })
})

describe('NALUStream', () => {
  const annexB = new Uint8Array([
    0x00, 0x00, 0x00, 0x01, 0x67, 0x64, 0x00, 0x1f, 0x00, 0x00, 0x00, 0x01, 0x68, 0xee, 0x3c, 0x80
  ])

  test('detects annexB and iterates packets', () => {
    const stream = new NALUStream(annexB, {} as never)

    expect(stream.type).toBe('annexB')
    expect(stream.boxSize).toBe(4)
    expect(stream.packetCount).toBe(2)
    expect(stream.packets).toHaveLength(2)

    const iterated = Array.from(stream)
    expect(iterated).toHaveLength(2)
    expect(iterated[0]?.[0]).toBe(0x67)

    const naluPairs = Array.from(stream.nalus())
    expect(naluPairs).toHaveLength(2)
    expect(naluPairs[0]?.rawNalu.length).toBeGreaterThan(naluPairs[0]?.nalu.length ?? 0)
  })

  test('converts annexB to packet and back', () => {
    const copy = new Uint8Array(annexB)
    const stream = new NALUStream(copy, { type: 'annexB', boxSize: 4 })

    stream.convertToPacket()
    expect(stream.type).toBe('packet')

    stream.convertToAnnexB()
    expect(stream.type).toBe('annexB')
  })

  test('works with packet streams and helpers', () => {
    const packet = new Uint8Array([
      0x00, 0x00, 0x00, 0x03, 0x65, 0x88, 0x99, 0x00, 0x00, 0x00, 0x02, 0x61, 0x00
    ])

    const stream = new NALUStream(packet, { type: 'packet', boxSize: 4 })
    expect(stream.packetCount).toBe(2)
    expect(stream.boxSizeMinusOne).toBe(3)

    expect(NALUStream.readUIntNBE(packet, 0, 4)).toBe(3)
    expect(NALUStream.array2hex(new Uint8Array([0, 10, 255]))).toBe('00 0a ff')

    const bad = stream.nextLengthCountedPacket(new Uint8Array([0, 0, 0, 1]), 0, 4)
    expect(bad.n).toBe(-2)
    expect(bad.message).toBe('bad length')
  })

  test('handles strict validation and unknown streams', () => {
    expect(
      () => new NALUStream(new Uint8Array([1, 2, 3, 4]), { type: 'invalid' as never })
    ).toThrow('type must be packet or annexB')

    expect(
      () =>
        new NALUStream(new Uint8Array([1, 2, 3, 4]), { type: 'packet', strict: true, boxSize: 1 })
    ).toThrow('invalid boxSize')

    const unknown = new NALUStream(new Uint8Array([9, 9, 9, 9]), { type: 'unknown' })
    expect(unknown.iterate()).toBe(0)

    expect(() =>
      new NALUStream(new Uint8Array([9, 9, 9, 9]), { type: 'unknown', strict: true }).getType(2)
    ).toThrow('cannot determine stream type or box size')
  })

  test('readUIntNBE requires box size', () => {
    expect(() => NALUStream.readUIntNBE(new Uint8Array([1, 2]), 0, 0)).toThrow('need a boxsize')
  })
})

describe('SPS', () => {
  test('throws for invalid NALU header and type', () => {
    expect(() => new SPS(new Uint8Array([0x80]))).toThrow('invalid NALU header')

    const notSps = new Uint8Array([0x65, 0x42, 0x00, 0x1e, 0xf4])
    expect(() => new SPS(notSps)).toThrow('SPS error: not SPS')
  })

  test('throws for unsupported profile idc', () => {
    const badProfile = new Uint8Array([0x67, 0x01, 0x00, 0x1e, 0x80])
    expect(() => new SPS(badProfile)).toThrow('invalid profile_idc')
  })

  test('parses a valid baseline SPS and exposes MIME', () => {
    const spsNalu = new Uint8Array([0x67, 0x42, 0x00, 0x1f, 0xe9, 0x01, 0x40, 0x7b, 0x20])

    const sps = new SPS(spsNalu)
    expect(sps.success).toBe(true)
    expect(sps.profileName).toBe('BASELINE')
    expect(sps.MIME.startsWith('avc1.')).toBe(true)
    expect(sps.stream.length).toBeGreaterThan(0)
  })
})
