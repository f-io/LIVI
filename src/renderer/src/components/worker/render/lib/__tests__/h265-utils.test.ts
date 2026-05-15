import { RawBitstream } from '../h264-utils'
import { HevcNaluTypes, HevcSPS, isHevcIrapNalType, readHevcNalType } from '../h265-utils'

/**
 * Build a synthetic HEVC SPS RBSP whose layout matches what HevcSPS expects to read.
 * Fields default to sane Main-profile values; callers can override the interesting bits.
 */
function buildHevcSps(
  opts: {
    width?: number
    height?: number
    chromaFormatIdc?: 0 | 1 | 2 | 3
    conformanceWindow?: { left: number; right: number; top: number; bottom: number }
    generalProfileSpace?: number
    generalTierFlag?: number
    generalProfileIdc?: number
    generalProfileCompatibility?: number
    generalConstraintHigh?: number
    generalConstraintLow?: number
    generalLevelIdc?: number
    spsMaxSubLayersMinus1?: number
  } = {}
): Uint8Array {
  const bs = new RawBitstream(2048)

  // NAL header
  bs.put_u_1(0) // forbidden_zero_bit
  bs.put_u(HevcNaluTypes.SPS, 6) // nal_unit_type = 33
  bs.put_u(0, 6) // nuh_layer_id
  bs.put_u(1, 3) // nuh_temporal_id_plus1

  // SPS prelude
  bs.put_u(0, 4) // sps_video_parameter_set_id
  const subMinus1 = opts.spsMaxSubLayersMinus1 ?? 0
  bs.put_u(subMinus1, 3) // sps_max_sub_layers_minus1
  bs.put_u_1(1) // sps_temporal_id_nesting_flag

  // profile_tier_level
  bs.put_u(opts.generalProfileSpace ?? 0, 2)
  bs.put_u_1(opts.generalTierFlag ?? 0)
  bs.put_u(opts.generalProfileIdc ?? 1, 5)
  bs.put_u(opts.generalProfileCompatibility ?? 0x60000000, 32)
  bs.put_u(opts.generalConstraintHigh ?? 0, 24)
  bs.put_u(opts.generalConstraintLow ?? 0, 24)
  bs.put_u(opts.generalLevelIdc ?? 120, 8) // general_level_idc

  // Sub-layer flags + level signalling
  if (subMinus1 > 0) {
    const subFlags: Array<{ profile: number; level: number }> = []
    for (let i = 0; i < subMinus1; i++) {
      bs.put_u_1(0) // profile flag
      bs.put_u_1(0) // level flag
      subFlags.push({ profile: 0, level: 0 })
    }
    for (let i = subMinus1; i < 8; i++) bs.put_u(0, 2)
    // None of the flags are 1 → no extra bits written
  }

  bs.put_ue_v(0) // sps_seq_parameter_set_id
  const chroma = opts.chromaFormatIdc ?? 1
  bs.put_ue_v(chroma)
  if (chroma === 3) bs.put_u_1(0) // separate_colour_plane_flag
  bs.put_ue_v(opts.width ?? 1920)
  bs.put_ue_v(opts.height ?? 1080)

  if (opts.conformanceWindow) {
    bs.put_u_1(1)
    bs.put_ue_v(opts.conformanceWindow.left)
    bs.put_ue_v(opts.conformanceWindow.right)
    bs.put_ue_v(opts.conformanceWindow.top)
    bs.put_ue_v(opts.conformanceWindow.bottom)
  } else {
    bs.put_u_1(0)
  }

  bs.put_ue_v(0) // bit_depth_luma_minus8
  bs.put_ue_v(0) // bit_depth_chroma_minus8

  bs.put_complete()
  // Return the underlying bytes (slice off the ptr-trailing padding bits — put_complete already truncated)
  return new Uint8Array(bs.buffer)
}

describe('readHevcNalType', () => {
  test('decodes the 6-bit NAL type from byte 0', () => {
    // nal_unit_type=33 (SPS) → upper byte = (33 << 1) = 0x42; lower bit is forbidden_zero
    expect(readHevcNalType(new Uint8Array([0x42]))).toBe(HevcNaluTypes.SPS)
    expect(readHevcNalType(new Uint8Array([(HevcNaluTypes.PPS << 1) & 0xff]))).toBe(
      HevcNaluTypes.PPS
    )
    expect(readHevcNalType(new Uint8Array([(HevcNaluTypes.IDR_W_RADL << 1) & 0xff]))).toBe(
      HevcNaluTypes.IDR_W_RADL
    )
  })

  test('returns -1 for an empty buffer', () => {
    expect(readHevcNalType(new Uint8Array(0))).toBe(-1)
  })
})

describe('isHevcIrapNalType', () => {
  test('IRAP range is [16, 23]', () => {
    expect(isHevcIrapNalType(15)).toBe(false)
    expect(isHevcIrapNalType(16)).toBe(true)
    expect(isHevcIrapNalType(HevcNaluTypes.IDR_W_RADL)).toBe(true)
    expect(isHevcIrapNalType(HevcNaluTypes.CRA_NUT)).toBe(true)
    expect(isHevcIrapNalType(23)).toBe(true)
    expect(isHevcIrapNalType(24)).toBe(false)
  })
})

describe('HevcSPS — parsing', () => {
  test('parses a Main-profile 1920×1080 SPS', () => {
    const sps = new HevcSPS(buildHevcSps())
    expect(sps.success).toBe(true)
    expect(sps.nal_unit_type).toBe(HevcNaluTypes.SPS)
    expect(sps.pic_width_in_luma_samples).toBe(1920)
    expect(sps.pic_height_in_luma_samples).toBe(1080)
    expect(sps.picWidth).toBe(1920)
    expect(sps.picHeight).toBe(1080)
    expect(sps.cropRect).toEqual({ x: 0, y: 0, width: 1920, height: 1080 })
  })

  test('throws if forbidden_zero_bit is 1', () => {
    const bytes = buildHevcSps()
    bytes[0] |= 0x80 // flip the top bit
    expect(() => new HevcSPS(bytes)).toThrow('NALU error: invalid NALU header')
  })

  test('throws if the NAL type is not SPS', () => {
    const bs = new RawBitstream(8)
    bs.put_u_1(0)
    bs.put_u(HevcNaluTypes.PPS, 6) // 34 — not SPS
    bs.put_u_1(0)
    bs.put_complete()
    expect(() => new HevcSPS(new Uint8Array(bs.buffer))).toThrow('SPS error: not HEVC SPS')
  })

  test('applies conformance-window crop (chroma 4:2:0 doubles offsets)', () => {
    const sps = new HevcSPS(
      buildHevcSps({
        width: 1920,
        height: 1088,
        conformanceWindow: { left: 0, right: 0, top: 0, bottom: 4 }
      })
    )
    // chroma_format_idc=1 → subW=2, subH=2
    // height_crop = 1088 - 2*(0 + 4) = 1080
    expect(sps.cropRect.height).toBe(1080)
    expect(sps.cropRect.width).toBe(1920)
  })

  test('handles chroma_format_idc=3 by also reading separate_colour_plane_flag', () => {
    const sps = new HevcSPS(buildHevcSps({ chromaFormatIdc: 3, width: 640, height: 480 }))
    expect(sps.chroma_format_idc).toBe(3)
    expect(sps.separate_colour_plane_flag).toBe(0)
    expect(sps.picWidth).toBe(640)
    expect(sps.picHeight).toBe(480)
  })

  test('chroma 4:2:2 (idc=2) uses subW=2 subH=1', () => {
    const sps = new HevcSPS(
      buildHevcSps({
        chromaFormatIdc: 2,
        width: 100,
        height: 100,
        conformanceWindow: { left: 1, right: 2, top: 1, bottom: 2 }
      })
    )
    // subW=2, subH=1: width crop = 100 - 2*3 = 94; height crop = 100 - 1*3 = 97
    expect(sps.cropRect.width).toBe(94)
    expect(sps.cropRect.height).toBe(97)
    expect(sps.cropRect.x).toBe(2)
    expect(sps.cropRect.y).toBe(1)
  })

  test('monochrome (chroma_format_idc=0) uses subW=1 subH=1', () => {
    const sps = new HevcSPS(
      buildHevcSps({
        chromaFormatIdc: 0,
        width: 320,
        height: 240,
        conformanceWindow: { left: 4, right: 4, top: 2, bottom: 2 }
      })
    )
    expect(sps.cropRect.width).toBe(320 - 8)
    expect(sps.cropRect.height).toBe(240 - 4)
  })

  test('skips a single sub-layer flag block', () => {
    expect(() => new HevcSPS(buildHevcSps({ spsMaxSubLayersMinus1: 2 }))).not.toThrow()
  })

  test('stream getter returns the underlying buffer', () => {
    const bytes = buildHevcSps()
    const sps = new HevcSPS(bytes)
    expect(sps.stream).toBeInstanceOf(Uint8Array)
  })
})

describe('HevcSPS — MIME string', () => {
  test('emits an hvc1 codec string for Main / L4.0', () => {
    const sps = new HevcSPS(
      buildHevcSps({
        generalProfileIdc: 1,
        generalProfileCompatibility: 0x60000000,
        generalTierFlag: 0,
        generalLevelIdc: 120 // 4.0 = 30 in spec; 120 is purely a test fixture
      })
    )
    const mime = sps.MIME
    expect(mime.startsWith('hvc1.')).toBe(true)
    // hvc1.<profile>.<compat>.<tier+level>.<≤6 constraint bytes>
    const parts = mime.split('.')
    expect(parts[0]).toBe('hvc1')
    expect(parts[1]).toBe('1') // profile_idc, no profile_space prefix
    expect(parts[3]).toBe('L120') // tier=L, level=120
  })

  test('high tier emits an H prefix on the level field', () => {
    const sps = new HevcSPS(buildHevcSps({ generalTierFlag: 1, generalLevelIdc: 153 }))
    expect(sps.MIME.split('.')[3]).toBe('H153')
  })

  test('profile_space ∈ {1,2,3} prefixes A/B/C onto the profile_idc', () => {
    const a = new HevcSPS(buildHevcSps({ generalProfileSpace: 1, generalProfileIdc: 2 }))
    expect(a.MIME.split('.')[1]).toBe('A2')
    const c = new HevcSPS(buildHevcSps({ generalProfileSpace: 3, generalProfileIdc: 4 }))
    expect(c.MIME.split('.')[1]).toBe('C4')
  })

  test('zero constraint bytes collapse to a single "B0" segment', () => {
    const sps = new HevcSPS(buildHevcSps({ generalConstraintHigh: 0, generalConstraintLow: 0 }))
    const last = sps.MIME.split('.').slice(4)
    expect(last).toEqual(['B0'])
  })

  test('non-zero constraint bytes are emitted hex-encoded, trailing zeros stripped', () => {
    const sps = new HevcSPS(
      buildHevcSps({
        generalConstraintHigh: 0x1234ab,
        generalConstraintLow: 0xff0000
      })
    )
    const parts = sps.MIME.split('.').slice(4)
    // High = 12 34 AB; Low = FF 00 00 → trailing zeros after FF are dropped → 12 34 AB FF
    expect(parts).toEqual(['12', '34', 'AB', 'FF'])
  })
})
