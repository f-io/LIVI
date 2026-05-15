import {
  encodeFrame,
  FRAME_HEADER_EXTENDED,
  FRAME_HEADER_SHORT,
  FrameParser,
  type RawFrame
} from '../codec'

const FLAG_FIRST = 0x01
const FLAG_LAST = 0x02
const BULK = FLAG_FIRST | FLAG_LAST

function collect(parser: FrameParser): RawFrame[] {
  const out: RawFrame[] = []
  parser.onFrame((f) => out.push(f))
  return out
}

describe('encodeFrame', () => {
  test('encodes channel/flags/payload-size + 2-byte msgId + data', () => {
    const buf = encodeFrame(3, BULK, 0x1234, Buffer.from([0xaa, 0xbb]))
    expect(buf[0]).toBe(3)
    expect(buf[1]).toBe(BULK)
    expect(buf.readUInt16BE(2)).toBe(4) // msgId(2) + data(2)
    expect(buf.readUInt16BE(4)).toBe(0x1234)
    expect(buf.subarray(6).equals(Buffer.from([0xaa, 0xbb]))).toBe(true)
    expect(buf.length).toBe(FRAME_HEADER_SHORT + 4)
  })

  test('encoded frame round-trips through FrameParser', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    parser.push(encodeFrame(7, BULK, 0xabcd, Buffer.from([1, 2, 3])))
    expect(frames).toHaveLength(1)
    expect(frames[0].channelId).toBe(7)
    expect(frames[0].msgId).toBe(0xabcd)
    expect(frames[0].payload.equals(Buffer.from([1, 2, 3]))).toBe(true)
  })
})

describe('FrameParser — single-frame (BULK)', () => {
  test('emits a frame when a complete BULK chunk arrives', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    parser.push(encodeFrame(1, BULK, 0x0001, Buffer.from('hello')))
    expect(frames).toHaveLength(1)
    expect(frames[0].payload.toString()).toBe('hello')
  })

  test('handles two frames pushed in one chunk', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    parser.push(
      Buffer.concat([
        encodeFrame(1, BULK, 0x0001, Buffer.from([1])),
        encodeFrame(2, BULK, 0x0002, Buffer.from([2, 2]))
      ])
    )
    expect(frames).toHaveLength(2)
    expect(frames[0].channelId).toBe(1)
    expect(frames[1].channelId).toBe(2)
    expect(frames[1].payload.equals(Buffer.from([2, 2]))).toBe(true)
  })

  test('reassembles a frame split across two TCP reads', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    const full = encodeFrame(1, BULK, 0x0042, Buffer.from('split'))
    parser.push(full.subarray(0, 3))
    expect(frames).toHaveLength(0)
    parser.push(full.subarray(3))
    expect(frames).toHaveLength(1)
    expect(frames[0].msgId).toBe(0x0042)
  })

  test('does not emit a frame for a payload shorter than 2 bytes (no msgId)', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    // hand-crafted BULK frame with 1-byte payload
    parser.push(Buffer.from([0x01, BULK, 0x00, 0x01, 0xff]))
    expect(frames).toHaveLength(0)
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('rawPayload includes the msgId; payload does not', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    parser.push(encodeFrame(1, BULK, 0x1234, Buffer.from([0x9])))
    expect(frames[0].rawPayload.length).toBe(3)
    expect(frames[0].rawPayload.readUInt16BE(0)).toBe(0x1234)
    expect(frames[0].payload.length).toBe(1)
  })
})

describe('FrameParser — multi-frame reassembly', () => {
  function makeFirstFrame(channelId: number, payload: Buffer, totalSize: number): Buffer {
    const header = Buffer.allocUnsafe(FRAME_HEADER_EXTENDED)
    header.writeUInt8(channelId, 0)
    header.writeUInt8(FLAG_FIRST, 1)
    header.writeUInt16BE(payload.length, 2)
    header.writeUInt32BE(totalSize, 4)
    return Buffer.concat([header, payload])
  }

  function makeContinuationFrame(channelId: number, flags: number, payload: Buffer): Buffer {
    const header = Buffer.allocUnsafe(FRAME_HEADER_SHORT)
    header.writeUInt8(channelId, 0)
    header.writeUInt8(flags, 1)
    header.writeUInt16BE(payload.length, 2)
    return Buffer.concat([header, payload])
  }

  test('FIRST + LAST reassembles into a single frame', () => {
    const parser = new FrameParser()
    const frames = collect(parser)

    const part1 = Buffer.from([0x00, 0x77, 0x01, 0x02, 0x03]) // msgId 0x0077 + 3 data bytes
    const part2 = Buffer.from([0x04, 0x05, 0x06])
    parser.push(makeFirstFrame(5, part1, part1.length + part2.length))
    expect(frames).toHaveLength(0)
    parser.push(makeContinuationFrame(5, FLAG_LAST, part2))
    expect(frames).toHaveLength(1)
    expect(frames[0].msgId).toBe(0x0077)
    expect(frames[0].payload.equals(Buffer.from([1, 2, 3, 4, 5, 6]))).toBe(true)
  })

  test('FIRST + MIDDLE + LAST reassembles in order', () => {
    const parser = new FrameParser()
    const frames = collect(parser)

    const a = Buffer.from([0x00, 0x09, 0xaa])
    const b = Buffer.from([0xbb, 0xcc])
    const c = Buffer.from([0xdd])
    parser.push(makeFirstFrame(2, a, a.length + b.length + c.length))
    parser.push(makeContinuationFrame(2, 0x00 /* middle */, b))
    parser.push(makeContinuationFrame(2, FLAG_LAST, c))

    expect(frames).toHaveLength(1)
    expect(frames[0].payload.equals(Buffer.from([0xaa, 0xbb, 0xcc, 0xdd]))).toBe(true)
  })

  test('reassemblies on different channels do not collide', () => {
    const parser = new FrameParser()
    const frames = collect(parser)

    const a1 = Buffer.from([0x00, 0x01, 0xaa])
    const a2 = Buffer.from([0xbb])
    const b1 = Buffer.from([0x00, 0x02, 0xcc])
    const b2 = Buffer.from([0xdd])

    parser.push(makeFirstFrame(1, a1, 4))
    parser.push(makeFirstFrame(2, b1, 4))
    parser.push(makeContinuationFrame(1, FLAG_LAST, a2))
    parser.push(makeContinuationFrame(2, FLAG_LAST, b2))

    expect(frames).toHaveLength(2)
    expect(frames[0].channelId).toBe(1)
    expect(frames[0].payload.equals(Buffer.from([0xaa, 0xbb]))).toBe(true)
    expect(frames[1].channelId).toBe(2)
    expect(frames[1].payload.equals(Buffer.from([0xcc, 0xdd]))).toBe(true)
  })

  test('continuation without a FIRST is dropped with a warning', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    parser.push(makeContinuationFrame(1, FLAG_LAST, Buffer.from([0, 0])))
    expect(frames).toHaveLength(0)
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('total-size mismatch on reassembly is warned but the frame is still emitted', () => {
    const parser = new FrameParser()
    const frames = collect(parser)
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})

    const a = Buffer.from([0x00, 0x09, 0xaa])
    const b = Buffer.from([0xbb])
    parser.push(makeFirstFrame(1, a, 999)) // wrong totalSize
    parser.push(makeContinuationFrame(1, FLAG_LAST, b))

    expect(frames).toHaveLength(1)
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })

  test('parser without an onFrame callback silently swallows complete frames', () => {
    const parser = new FrameParser()
    expect(() => {
      parser.push(encodeFrame(1, BULK, 0x0001, Buffer.from([1, 2])))
    }).not.toThrow()
  })
})
