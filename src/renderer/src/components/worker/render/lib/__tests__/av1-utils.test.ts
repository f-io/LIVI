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
  // Optional flag paths the spec allows. Defaults stay minimal (matches the
  // shape an AA-wireless stream actually carries).
  reducedStillPicture?: boolean
  timingInfo?: {
    equalPictureInterval?: boolean
    /** uvlc leading-zero count, only used when equalPictureInterval=true */
    uvlcLeading?: number
    decoderModelInfo?: boolean
    opModelPresent?: boolean
  }
  initialDisplayDelay?: boolean
  initialDelayPerOp?: boolean
  operatingPointsCntMinus1?: number
  frameIdNumbersPresent?: boolean
  enableOrderHint?: boolean
  seqChooseScreenDetection?: boolean
  seqForceScreenDetection?: 0 | 1
  seqChooseIntegerMv?: boolean
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
  const reducedStill = opts.reducedStillPicture ? 1 : 0
  b.put(reducedStill, 1)

  if (reducedStill) {
    b.put(level, 5) // seq_level_idx_0
  } else {
    const tiPresent = opts.timingInfo ? 1 : 0
    b.put(tiPresent, 1)
    if (tiPresent) {
      // Non-zero filler — Bitstream.deemulate() eats 0x03 after 0x00 0x00 and
      // would silently corrupt 32 zero bits inside an AV1 SPS payload.
      b.put(1, 32) // num_units_in_display_tick
      b.put(1, 32) // time_scale
      const epi = opts.timingInfo!.equalPictureInterval ? 1 : 0
      b.put(epi, 1)
      if (epi) {
        // uvlc: <leading zeros> 1 [leading-zero count bits]
        const leading = opts.timingInfo!.uvlcLeading ?? 0
        for (let i = 0; i < leading; i++) b.put(0, 1)
        b.put(1, 1)
        if (leading > 0) b.put(1, leading)
      }
      const dmiPresent = opts.timingInfo!.decoderModelInfo ? 1 : 0
      b.put(dmiPresent, 1)
      if (dmiPresent) {
        b.put(1, 5) // buffer_delay_length_minus_1
        b.put(1, 32)
        b.put(1, 10)
        b.put(1, 5)
      }
    }

    const idd = opts.initialDisplayDelay ? 1 : 0
    b.put(idd, 1)

    const opCnt = opts.operatingPointsCntMinus1 ?? 0
    b.put(opCnt, 5)

    for (let i = 0; i <= opCnt; i++) {
      b.put(0, 12) // operating_point_idc[i]
      // Only op[0] uses the user-supplied level/tier; later ops use 0/0.
      const opLevel = i === 0 ? level : 0
      const opTier = i === 0 ? tier : 0
      b.put(opLevel, 5)
      if (opLevel > 7) b.put(opTier, 1)

      if (opts.timingInfo?.decoderModelInfo) {
        const opModel = i === 0 && opts.timingInfo.opModelPresent ? 1 : 0
        b.put(opModel, 1)
        if (opModel) {
          // Non-zero filler — see comment above re: deemulate.
          b.put(1, 32)
          b.put(1, 32)
          b.put(1, 1)
        }
      }
      if (idd) {
        const present = i === 0 && opts.initialDelayPerOp ? 1 : 0
        b.put(present, 1)
        if (present) b.put(0, 4)
      }
    }
  }

  const fwBits = Math.max(bitsForValue(opts.width - 1), 1)
  const fhBits = Math.max(bitsForValue(opts.height - 1), 1)
  b.put(fwBits - 1, 4) // frame_width_bits_minus_1
  b.put(fhBits - 1, 4) // frame_height_bits_minus_1
  b.put(opts.width - 1, fwBits)
  b.put(opts.height - 1, fhBits)

  if (!reducedStill) {
    const fidPresent = opts.frameIdNumbersPresent ? 1 : 0
    b.put(fidPresent, 1)
    if (fidPresent) {
      b.put(0, 4) // delta_frame_id_length_minus_2
      b.put(0, 3) // additional_frame_id_length_minus_1
    }
  }

  b.put(0, 1) // use_128x128_superblock
  b.put(0, 1) // enable_filter_intra
  b.put(0, 1) // enable_intra_edge_filter

  if (!reducedStill) {
    b.put(0, 1) // enable_interintra_compound
    b.put(0, 1) // enable_masked_compound
    b.put(0, 1) // enable_warped_motion
    b.put(0, 1) // enable_dual_filter

    const orderHint = opts.enableOrderHint ? 1 : 0
    b.put(orderHint, 1)
    if (orderHint) {
      b.put(0, 1) // enable_jnt_comp
      b.put(0, 1) // enable_ref_frame_mvs
    }

    const chooseScreen = opts.seqChooseScreenDetection ?? true
    b.put(chooseScreen ? 1 : 0, 1)
    let forceScreen = 0
    if (chooseScreen) {
      forceScreen = 2
    } else {
      forceScreen = opts.seqForceScreenDetection ?? 0
      b.put(forceScreen, 1)
    }
    if (forceScreen > 0) {
      const chooseIntegerMv = opts.seqChooseIntegerMv ?? true
      b.put(chooseIntegerMv ? 1 : 0, 1)
      if (!chooseIntegerMv) b.put(0, 1) // seq_force_integer_mv
    }
    if (orderHint) b.put(0, 3) // order_hint_bits_minus_1
  }

  b.put(0, 1) // enable_superres
  b.put(0, 1) // enable_cdef
  b.put(0, 1) // enable_restoration

  // color_config()
  const highBitdepth = bitDepth > 8 ? 1 : 0
  b.put(highBitdepth, 1)
  if (profile === 2 && highBitdepth) b.put(bitDepth === 12 ? 1 : 0, 1)
  if (profile !== 1) b.put(0, 1) // monochrome = 0

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

  test('readLeb128 falls back at the 8-byte limit when no terminator bit', () => {
    // Eight 0x80 bytes — all continuation bits set, no terminator. Loop exits
    // at i===8 and returns the accumulated value without a clean stop bit.
    const buf = new Uint8Array([0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80])
    const got = readLeb128(buf, 0)
    expect(got).not.toBeNull()
    expect(got?.length).toBe(8)
  })

  test('readLeb128 returns null when the buffer ends mid-stream', () => {
    // First byte has continuation bit set, but buffer is exactly 1 byte long
    expect(readLeb128(new Uint8Array([0x80]), 0)).toBeNull()
  })

  test('parseAv1Obus stops on forbidden_bit', () => {
    expect([...parseAv1Obus(new Uint8Array([0x80]))]).toEqual([])
  })

  test('parseAv1Obus parses an OBU with the extension flag set', () => {
    // type=1 (SEQUENCE_HEADER), extension_flag=1, has_size_field=1
    // header byte = 0b00001110, then extension byte (temporal_id<<5)|(spatial_id<<3) | reserved
    const ext = (4 << 5) | (2 << 3) // temporal_id=4, spatial_id=2
    const payload = new Uint8Array([0xaa, 0xbb])
    const obu = new Uint8Array([0b00001110, ext, payload.length, ...payload])
    const got = [...parseAv1Obus(obu)]
    expect(got).toHaveLength(1)
    expect(got[0]!.type).toBe(Av1ObuType.SEQUENCE_HEADER)
    expect(got[0]!.extensionFlag).toBe(true)
    expect(got[0]!.temporalId).toBe(4)
    expect(got[0]!.spatialId).toBe(2)
    expect(Array.from(got[0]!.payload)).toEqual([0xaa, 0xbb])
  })

  test('parseAv1Obus handles an OBU without size_field (payload runs to end)', () => {
    // type=1, extension_flag=0, has_size_field=0 → header byte = 0b00001000
    const obu = new Uint8Array([0b00001000, 0x11, 0x22, 0x33])
    const got = [...parseAv1Obus(obu)]
    expect(got).toHaveLength(1)
    expect(got[0]!.hasSizeField).toBe(false)
    expect(Array.from(got[0]!.payload)).toEqual([0x11, 0x22, 0x33])
  })

  test('parseAv1Obus truncates a payload that overshoots the buffer', () => {
    // type=1, has_size_field=1, declared payload length = 99 but buffer only has 3 bytes
    const obu = new Uint8Array([0b00001010, 99, 0xde, 0xad, 0xbe])
    const got = [...parseAv1Obus(obu)]
    expect(got).toHaveLength(1)
    expect(got[0]!.payload.length).toBe(3)
  })

  test('parseAv1Obus stops cleanly when extension flag has no follow-up byte', () => {
    // header says extension=1 but buffer ends right after
    expect([...parseAv1Obus(new Uint8Array([0b00001110]))]).toEqual([])
  })

  test('Av1SequenceHeader for seq_profile=1 forces monochrome=0 without reading a bit', () => {
    const payload = buildSequenceHeaderPayload({ profile: 1, width: 800, height: 600 })
    const hdr = new Av1SequenceHeader(payload)
    expect(hdr.seq_profile).toBe(1)
    expect(hdr.monochrome).toBe(0)
  })

  test('isAv1KeyFrame returns false on an empty buffer', () => {
    expect(isAv1KeyFrame(new Uint8Array(0))).toBe(false)
  })

  test('containsAv1SequenceHeader returns false on a frame-only stream', () => {
    const frame = buildFrameObu(Av1FrameType.INTER_FRAME, new Uint8Array([0]))
    expect(containsAv1SequenceHeader(frame)).toBe(false)
  })

  // ── Optional sequence-header paths ──────────────────────────────────────

  test('Av1SequenceHeader handles reduced_still_picture_header', () => {
    const payload = buildSequenceHeaderPayload({
      reducedStillPicture: true,
      level: 6,
      width: 1920,
      height: 1080
    })
    const hdr = new Av1SequenceHeader(payload)
    expect(hdr.reduced_still_picture_header).toBe(1)
    expect(hdr.seq_level_idx_0).toBe(6)
    expect(hdr.seq_tier_0).toBe(0)
    expect(hdr.max_frame_width).toBe(1920)
    expect(hdr.max_frame_height).toBe(1080)
  })

  test('Av1SequenceHeader walks timing_info_present without equal_picture_interval', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      timingInfo: { equalPictureInterval: false }
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks timing_info_present + equal_picture_interval (uvlc=0)', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      timingInfo: { equalPictureInterval: true, uvlcLeading: 0 }
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks the uvlc skip with leading zeros', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      timingInfo: { equalPictureInterval: true, uvlcLeading: 3 }
    })
    // Drives the `if (leading > 0) bs.u(leading)` branch in skipUvlc
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks decoder_model_info_present (without op-level model)', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      timingInfo: { decoderModelInfo: true }
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks operating-point decoder model present', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      timingInfo: { decoderModelInfo: true, opModelPresent: true }
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks initial_display_delay_present (per-op flag set)', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      initialDisplayDelay: true,
      initialDelayPerOp: true
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks multi-op signalling (cnt > 0)', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      operatingPointsCntMinus1: 2
    })
    const hdr = new Av1SequenceHeader(payload)
    // op[0] keeps user-supplied level (8 by default)
    expect(hdr.seq_level_idx_0).toBe(8)
  })

  test('Av1SequenceHeader walks frame_id_numbers_present', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      frameIdNumbersPresent: true
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks enable_order_hint + order_hint_bits_minus_1', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      enableOrderHint: true
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks seq_force_screen_detection=0 (no integer_mv read)', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      seqChooseScreenDetection: false,
      seqForceScreenDetection: 0
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader walks seq_force_screen_detection=1 + force_integer_mv', () => {
    const payload = buildSequenceHeaderPayload({
      width: 1280,
      height: 720,
      seqChooseScreenDetection: false,
      seqForceScreenDetection: 1,
      seqChooseIntegerMv: false
    })
    expect(new Av1SequenceHeader(payload).success).toBe(true)
  })

  test('Av1SequenceHeader parses profile=2 + 12-bit color depth', () => {
    const payload = buildSequenceHeaderPayload({
      profile: 2,
      bitDepth: 12,
      level: 13,
      tier: 1,
      width: 7680,
      height: 4320
    })
    const hdr = new Av1SequenceHeader(payload)
    expect(hdr.bit_depth).toBe(12)
    expect(hdr.twelve_bit).toBe(1)
  })

  test('getAv1DecoderConfig catches and logs a parser throw on truncated SEQUENCE_HEADER', () => {
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    // SEQUENCE_HEADER OBU with declared payload length 1 but only 1 byte of
    // payload — the parser bumps past `max` and Bitstream throws.
    const obu = new Uint8Array([0b00001010, 1, 0x00])
    expect(getAv1DecoderConfig(obu)).toBeNull()
    warn.mockRestore()
  })
})
