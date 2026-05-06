import {
  Av1FrameType,
  Av1ObuType,
  Av1SequenceHeader,
  containsAv1SequenceHeader,
  findAv1Obu,
  getAv1DecoderConfig,
  isAv1KeyFrame,
  parseAv1Obus,
  readLeb128
} from '../av1-utils'

class BitBuilder {
  private bits: number[] = []
  put(value: number, count: number): void {
    for (let i = count - 1; i >= 0; i--) this.bits.push((value >>> i) & 1)
  }
  toBytes(): Uint8Array {
    const bytes = new Uint8Array((this.bits.length + 7) >>> 3)
    for (let i = 0; i < this.bits.length; i++) {
      bytes[i >>> 3] |= this.bits[i]! << (7 - (i & 7))
    }
    return bytes
  }
}

function leb128(value: number): Uint8Array {
  const out: number[] = []
  let v = value >>> 0
  do {
    let byte = v & 0x7f
    v >>>= 7
    if (v !== 0) byte |= 0x80
    out.push(byte)
  } while (v !== 0)
  return new Uint8Array(out)
}

function bitsForValue(v: number): number {
  if (v === 0) return 1
  let n = 0
  let x = v
  while (x > 0) {
    n++
    x >>>= 1
  }
  return n
}

interface SeqOpts {
  profile?: 0 | 1 | 2
  level?: number
  tier?: 0 | 1
  bitDepth?: 8 | 10 | 12
  width: number
  height: number
}

// Build a minimal AV1 sequence_header_obu payload (not the OBU header itself).
function buildSequenceHeaderPayload(opts: SeqOpts): Uint8Array {
  const profile = opts.profile ?? 0
  const level = opts.level ?? 8 // 4.0
  const tier = opts.tier ?? 0
  const bitDepth = opts.bitDepth ?? 8

  const b = new BitBuilder()
  b.put(profile, 3)
  b.put(0, 1) // still_picture
  b.put(0, 1) // reduced_still_picture_header

  // !reduced path
  b.put(0, 1) // timing_info_present_flag
  b.put(0, 1) // initial_display_delay_present_flag
  b.put(0, 5) // operating_points_cnt_minus_1 = 0
  // op[0]:
  b.put(0, 12) // operating_point_idc
  b.put(level, 5)
  if (level > 7) b.put(tier, 1)

  const fwBits = Math.max(bitsForValue(opts.width - 1), 1)
  const fhBits = Math.max(bitsForValue(opts.height - 1), 1)
  b.put(fwBits - 1, 4) // frame_width_bits_minus_1
  b.put(fhBits - 1, 4) // frame_height_bits_minus_1
  b.put(opts.width - 1, fwBits)
  b.put(opts.height - 1, fhBits)

  b.put(0, 1) // frame_id_numbers_present_flag

  b.put(0, 1) // use_128x128_superblock
  b.put(0, 1) // enable_filter_intra
  b.put(0, 1) // enable_intra_edge_filter

  // !reduced path continued
  b.put(0, 1) // enable_interintra_compound
  b.put(0, 1) // enable_masked_compound
  b.put(0, 1) // enable_warped_motion
  b.put(0, 1) // enable_dual_filter
  b.put(0, 1) // enable_order_hint = 0 → skip jnt_comp / ref_frame_mvs / order_hint_bits
  b.put(1, 1) // seq_choose_screen_detection_tools = 1 → seq_force = SELECT (2)
  // seq_force_screen_detection_tools > 0
  b.put(1, 1) // seq_choose_integer_mv = 1 → no seq_force_integer_mv

  b.put(0, 1) // enable_superres
  b.put(0, 1) // enable_cdef
  b.put(0, 1) // enable_restoration

  // color_config()
  const highBitdepth = bitDepth > 8 ? 1 : 0
  b.put(highBitdepth, 1)
  if (profile === 2 && highBitdepth) b.put(bitDepth === 12 ? 1 : 0, 1)
  if (profile !== 1) b.put(0, 1) // monochrome = 0
  // We don't need to encode further — parser stops after monochrome.

  return b.toBytes()
}

function buildSequenceHeaderObu(opts: SeqOpts): Uint8Array {
  const payload = buildSequenceHeaderPayload(opts)
  const obuHeader = 0b00001010 // type=1 (SEQUENCE_HEADER), has_size_field=1
  const size = leb128(payload.length)
  return new Uint8Array([obuHeader, ...size, ...payload])
}

function buildFrameObu(frameType: Av1FrameType, payload: Uint8Array): Uint8Array {
  const obuHeader = 0b00110010 // type=6 (FRAME), has_size_field=1
  // Prepend show_existing_frame=0 (1 bit) + frame_type (2 bits) + 5 padding bits
  const head = new Uint8Array(1)
  head[0] = ((0 & 1) << 7) | ((frameType & 0x3) << 5)
  const merged = new Uint8Array(head.length + payload.length)
  merged.set(head, 0)
  merged.set(payload, head.length)
  const size = leb128(merged.length)
  return new Uint8Array([obuHeader, ...size, ...merged])
}

describe('av1-utils', () => {
  test('readLeb128 decodes single-byte values', () => {
    expect(readLeb128(new Uint8Array([0]), 0)).toEqual({ value: 0, length: 1 })
    expect(readLeb128(new Uint8Array([0x7f]), 0)).toEqual({ value: 127, length: 1 })
  })

  test('readLeb128 decodes multi-byte values', () => {
    expect(readLeb128(new Uint8Array([0x80, 0x01]), 0)).toEqual({ value: 128, length: 2 })
    expect(readLeb128(new Uint8Array([0xe5, 0x8e, 0x26]), 0)).toEqual({
      value: 624485,
      length: 3
    })
  })

  test('readLeb128 returns null on out-of-range offset', () => {
    expect(readLeb128(new Uint8Array([0x80]), 5)).toBeNull()
  })

  test('parseAv1Obus walks two OBUs in order', () => {
    const seq = buildSequenceHeaderObu({ width: 1280, height: 720 })
    const frame = buildFrameObu(Av1FrameType.KEY_FRAME, new Uint8Array([0]))
    const stream = new Uint8Array([...seq, ...frame])
    const obus = [...parseAv1Obus(stream)]
    expect(obus).toHaveLength(2)
    expect(obus[0]!.type).toBe(Av1ObuType.SEQUENCE_HEADER)
    expect(obus[1]!.type).toBe(Av1ObuType.FRAME)
  })

  test('findAv1Obu returns matching OBU', () => {
    const seq = buildSequenceHeaderObu({ width: 1920, height: 1080 })
    const found = findAv1Obu(seq, Av1ObuType.SEQUENCE_HEADER)
    expect(found).not.toBeNull()
    expect(found?.type).toBe(Av1ObuType.SEQUENCE_HEADER)
  })

  test('findAv1Obu returns null when type missing', () => {
    const seq = buildSequenceHeaderObu({ width: 1920, height: 1080 })
    expect(findAv1Obu(seq, Av1ObuType.FRAME)).toBeNull()
  })

  test('isAv1KeyFrame detects FRAME with KEY_FRAME', () => {
    const frame = buildFrameObu(Av1FrameType.KEY_FRAME, new Uint8Array([0]))
    expect(isAv1KeyFrame(frame)).toBe(true)
  })

  test('isAv1KeyFrame returns false on inter frame', () => {
    const frame = buildFrameObu(Av1FrameType.INTER_FRAME, new Uint8Array([0]))
    expect(isAv1KeyFrame(frame)).toBe(false)
  })

  test('containsAv1SequenceHeader is true when SEQUENCE_HEADER OBU present', () => {
    const seq = buildSequenceHeaderObu({ width: 1920, height: 1080 })
    expect(containsAv1SequenceHeader(seq)).toBe(true)
  })

  test('Av1SequenceHeader parses 1920x1080 profile 0 8-bit', () => {
    const payload = buildSequenceHeaderPayload({ width: 1920, height: 1080 })
    const hdr = new Av1SequenceHeader(payload)
    expect(hdr.success).toBe(true)
    expect(hdr.seq_profile).toBe(0)
    expect(hdr.seq_level_idx_0).toBe(8)
    expect(hdr.seq_tier_0).toBe(0)
    expect(hdr.bit_depth).toBe(8)
    expect(hdr.max_frame_width).toBe(1920)
    expect(hdr.max_frame_height).toBe(1080)
    expect(hdr.MIME).toBe('av01.0.08M.08')
  })

  test('Av1SequenceHeader parses profile 2 with 10-bit', () => {
    const payload = buildSequenceHeaderPayload({
      profile: 2,
      level: 12,
      tier: 1,
      bitDepth: 10,
      width: 3840,
      height: 2160
    })
    const hdr = new Av1SequenceHeader(payload)
    expect(hdr.seq_profile).toBe(2)
    expect(hdr.bit_depth).toBe(10)
    expect(hdr.max_frame_width).toBe(3840)
    expect(hdr.max_frame_height).toBe(2160)
    expect(hdr.MIME).toBe('av01.2.12H.10')
  })

  test('getAv1DecoderConfig returns config for stream containing SEQUENCE_HEADER', () => {
    const stream = buildSequenceHeaderObu({ width: 1280, height: 720 })
    const cfg = getAv1DecoderConfig(stream)
    expect(cfg).not.toBeNull()
    expect(cfg?.codec).toMatch(/^av01\./)
    expect(cfg?.codedWidth).toBe(1280)
    expect(cfg?.codedHeight).toBe(720)
  })

  test('getAv1DecoderConfig returns null when no SEQUENCE_HEADER present', () => {
    const frame = buildFrameObu(Av1FrameType.KEY_FRAME, new Uint8Array([0]))
    expect(getAv1DecoderConfig(frame)).toBeNull()
  })
})
