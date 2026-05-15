import { RawBitstream } from '../h264-utils'
import { HevcNaluTypes } from '../h265-utils'
import {
  containsParameterSet,
  getDecoderConfig,
  getNaluFromStream,
  isKeyFrame,
  NaluTypes
} from '../utils'

const annexB = new Uint8Array([
  0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1f, 0xe9, 0x01, 0x40, 0x7b, 0x20, 0x00, 0x00, 0x00,
  0x01, 0x65, 0x88, 0x84, 0x21
])

function buildHevcSpsNalu(width = 1280, height = 720): Uint8Array {
  const bs = new RawBitstream(2048)
  bs.put_u_1(0) // forbidden_zero_bit
  bs.put_u(HevcNaluTypes.SPS, 6) // nal_unit_type = 33
  bs.put_u(0, 6) // nuh_layer_id
  bs.put_u(1, 3) // nuh_temporal_id_plus1

  bs.put_u(0, 4) // sps_video_parameter_set_id
  bs.put_u(0, 3) // sps_max_sub_layers_minus1
  bs.put_u_1(1) // sps_temporal_id_nesting_flag

  bs.put_u(0, 2) // profile_space
  bs.put_u_1(0) // tier
  bs.put_u(1, 5) // profile_idc = Main
  bs.put_u(0x60000000, 32) // compatibility flags
  bs.put_u(0, 24) // constraint high
  bs.put_u(0, 24) // constraint low
  bs.put_u(120, 8) // level_idc

  bs.put_ue_v(0) // seq_parameter_set_id
  bs.put_ue_v(1) // chroma_format_idc = 4:2:0
  bs.put_ue_v(width)
  bs.put_ue_v(height)
  bs.put_u_1(0) // conformance_window_flag = 0
  bs.put_ue_v(0) // bit_depth_luma_minus8
  bs.put_ue_v(0) // bit_depth_chroma_minus8

  bs.put_complete()
  return new Uint8Array(bs.buffer)
}

function wrapAnnexB(...nalus: Uint8Array[]): Uint8Array {
  const out: number[] = []
  for (const n of nalus) {
    out.push(0, 0, 0, 1, ...n)
  }
  return new Uint8Array(out)
}

describe('render/lib/utils', () => {
  test('extracts NALU by type from annexB and packet streams', () => {
    const sps = getNaluFromStream(annexB, NaluTypes.SPS, 'annexB')
    expect(sps).not.toBeNull()
    expect(sps?.type).toBe(NaluTypes.SPS)

    const packet = new Uint8Array([
      0x00, 0x00, 0x00, 0x09, 0x67, 0x42, 0x00, 0x1f, 0xe9, 0x01, 0x40, 0x7b, 0x20
    ])

    const spsPacket = getNaluFromStream(packet, NaluTypes.SPS, 'packet')
    expect(spsPacket).not.toBeNull()
  })

  test('returns null if stream has no requested nalu', () => {
    const res = getNaluFromStream(new Uint8Array([0, 0, 0, 1, 1, 2, 3, 4]), NaluTypes.PPS)
    expect(res).toBeNull()
  })

  test('detects keyframes using IDR nalu', () => {
    expect(isKeyFrame(annexB)).toBe(true)
    expect(isKeyFrame(new Uint8Array([0, 0, 0, 1, 0x61, 1, 2, 3]))).toBe(false)
  })

  test('builds decoder config from SPS and falls back on invalid input', () => {
    const cfg = getDecoderConfig(annexB)
    expect(cfg).not.toBeNull()
    expect(cfg?.codec.startsWith('avc1.')).toBe(true)
    expect(cfg?.codedWidth).toBeGreaterThan(0)
    expect(cfg?.codedHeight).toBeGreaterThan(0)

    expect(getDecoderConfig(new Uint8Array([1, 2, 3, 4]))).toBeNull()
  })

  test('builds an h265 decoder config from a HEVC SPS', () => {
    const stream = wrapAnnexB(buildHevcSpsNalu(1280, 720))
    const cfg = getDecoderConfig(stream, 'h265')
    expect(cfg).not.toBeNull()
    expect(cfg?.codec.startsWith('hvc1.')).toBe(true)
    expect(cfg?.codedWidth).toBe(1280)
    expect(cfg?.codedHeight).toBe(720)
  })

  test('getDecoderConfig logs and falls back when both stream types fail', () => {
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    // A trailing 0x00 0x00 0x00 0x01 followed by a one-byte NALU isn't a valid SPS
    expect(getDecoderConfig(new Uint8Array([0, 0, 0, 1, 0xff, 0xff]), 'h265')).toBeNull()
    warn.mockRestore()
  })

  test('isKeyFrame detects HEVC IRAP NAL types', () => {
    // IDR_W_RADL = 19 → header byte = 19<<1 = 0x26
    const idrNalu = new Uint8Array([0x26, 0x00])
    expect(isKeyFrame(wrapAnnexB(idrNalu), 'h265')).toBe(true)

    // TRAIL_N = 0 → header byte = 0
    const trail = new Uint8Array([0x00, 0x00])
    expect(isKeyFrame(wrapAnnexB(trail), 'h265')).toBe(false)
  })

  test('isKeyFrame returns false for an HEVC stream that has no NALUs', () => {
    expect(isKeyFrame(new Uint8Array([0, 0, 0, 1]), 'h265')).toBe(false)
  })

  test('containsParameterSet detects HEVC SPS', () => {
    const stream = wrapAnnexB(buildHevcSpsNalu())
    expect(containsParameterSet(stream, 'h265')).toBe(true)
  })

  test('containsParameterSet ignores HEVC non-PS NALUs', () => {
    // TRAIL_R = 1 → header byte = 0x02 (long enough to pass the minLen guard)
    const trail = new Uint8Array([0x02, 0x00, 0x01, 0x02])
    expect(containsParameterSet(wrapAnnexB(trail), 'h265')).toBe(false)
  })

  test('containsParameterSet detects H.264 SPS', () => {
    expect(containsParameterSet(annexB, 'h264')).toBe(true)
  })

  test('containsParameterSet is always false for VP9 (self-contained frames)', () => {
    expect(containsParameterSet(new Uint8Array([0x82, 0x49, 0x83, 0x42]), 'vp9')).toBe(false)
  })
})
