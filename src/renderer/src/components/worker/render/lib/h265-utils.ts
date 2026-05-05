// HEVC (H.265) bitstream utilities

import { Bitstream, NALUStream, RawBitstream, type StreamType } from './h264-utils'

export { Bitstream, NALUStream, RawBitstream, type StreamType }

export enum HevcNaluTypes {
  TRAIL_N = 0,
  TRAIL_R = 1,
  TSA_N = 2,
  TSA_R = 3,
  STSA_N = 4,
  STSA_R = 5,
  RADL_N = 6,
  RADL_R = 7,
  RASL_N = 8,
  RASL_R = 9,
  BLA_W_LP = 16,
  BLA_W_RADL = 17,
  BLA_N_LP = 18,
  IDR_W_RADL = 19,
  IDR_N_LP = 20,
  CRA_NUT = 21,
  RSV_IRAP_VCL22 = 22,
  RSV_IRAP_VCL23 = 23,
  VPS = 32,
  SPS = 33,
  PPS = 34,
  AUD = 35,
  EOS = 36,
  EOB = 37,
  FD = 38,
  PREFIX_SEI = 39,
  SUFFIX_SEI = 40
}

export function readHevcNalType(nalu: Uint8Array): number {
  if (nalu.length < 1) return -1
  return (nalu[0]! >> 1) & 0x3f
}

// IRAP = Intra Random Access Point. Any NAL in [16,23] decodes from-scratch
// (BLA/IDR/CRA). Treated as keyframe for our purposes.
export function isHevcIrapNalType(t: number): boolean {
  return t >= 16 && t <= 23
}

function byte2hex(v: number): string {
  return ('00' + (v & 0xff).toString(16)).slice(-2)
}

function reverseBits32(v: number): number {
  let r = 0
  let x = v >>> 0
  for (let i = 0; i < 32; i++) {
    r = ((r << 1) | (x & 1)) >>> 0
    x >>>= 1
  }
  return r >>> 0
}

export class HevcSPS {
  bitstream: Bitstream

  forbidden_zero_bit: number
  nal_unit_type: number
  nuh_layer_id: number
  nuh_temporal_id_plus1: number

  sps_video_parameter_set_id: number
  sps_max_sub_layers_minus1: number
  sps_temporal_id_nesting_flag: number

  general_profile_space: number
  general_tier_flag: number
  general_profile_idc: number
  general_profile_compatibility_flags: number
  general_constraint_indicator_flags_high: number
  general_constraint_indicator_flags_low: number
  general_level_idc: number

  sps_seq_parameter_set_id: number
  chroma_format_idc: number
  separate_colour_plane_flag: number
  pic_width_in_luma_samples: number
  pic_height_in_luma_samples: number
  conformance_window_flag: number
  conf_win_left_offset: number
  conf_win_right_offset: number
  conf_win_top_offset: number
  conf_win_bottom_offset: number
  bit_depth_luma_minus8: number
  bit_depth_chroma_minus8: number

  picWidth: number
  picHeight: number
  cropRect: { x: number; y: number; width: number; height: number }
  success: boolean

  constructor(SPS: Uint8Array) {
    const bs = new Bitstream(SPS)
    this.bitstream = bs

    this.forbidden_zero_bit = bs.u_1()
    if (this.forbidden_zero_bit) throw new Error('NALU error: invalid NALU header')
    this.nal_unit_type = bs.u(6)
    if (this.nal_unit_type !== HevcNaluTypes.SPS) throw new Error('SPS error: not HEVC SPS')
    this.nuh_layer_id = bs.u(6)
    this.nuh_temporal_id_plus1 = bs.u(3)

    this.sps_video_parameter_set_id = bs.u(4)
    this.sps_max_sub_layers_minus1 = bs.u(3)
    this.sps_temporal_id_nesting_flag = bs.u_1()

    // profile_tier_level(profilePresent=1, maxNumSubLayersMinus1)
    this.general_profile_space = bs.u(2)
    this.general_tier_flag = bs.u_1()
    this.general_profile_idc = bs.u(5)
    let pcf = 0
    for (let i = 0; i < 32; i++) pcf = ((pcf << 1) | bs.u_1()) >>> 0
    this.general_profile_compatibility_flags = pcf >>> 0
    let cfHi = 0
    for (let i = 0; i < 24; i++) cfHi = ((cfHi << 1) | bs.u_1()) >>> 0
    let cfLo = 0
    for (let i = 0; i < 24; i++) cfLo = ((cfLo << 1) | bs.u_1()) >>> 0
    this.general_constraint_indicator_flags_high = cfHi >>> 0
    this.general_constraint_indicator_flags_low = cfLo >>> 0
    this.general_level_idc = bs.u_8()!

    // Sub-layer flags + level signalling — values are not used downstream,
    // we only need to advance the pointer past them.
    if (this.sps_max_sub_layers_minus1 > 0) {
      const subFlags: Array<{ profile: number; level: number }> = []
      for (let i = 0; i < this.sps_max_sub_layers_minus1; i++) {
        subFlags.push({ profile: bs.u_1(), level: bs.u_1() })
      }
      for (let i = this.sps_max_sub_layers_minus1; i < 8; i++) bs.u(2)
      for (const f of subFlags) {
        if (f.profile) {
          // 2+1+5+32+4+43+1 = 88 bits
          for (let i = 0; i < 88; i++) bs.u_1()
        }
        if (f.level) bs.u_8()
      }
    }

    this.sps_seq_parameter_set_id = bs.ue_v()
    this.chroma_format_idc = bs.ue_v()
    this.separate_colour_plane_flag = 0
    if (this.chroma_format_idc === 3) {
      this.separate_colour_plane_flag = bs.u_1()
    }
    this.pic_width_in_luma_samples = bs.ue_v()
    this.pic_height_in_luma_samples = bs.ue_v()
    this.conformance_window_flag = bs.u_1()
    this.conf_win_left_offset = 0
    this.conf_win_right_offset = 0
    this.conf_win_top_offset = 0
    this.conf_win_bottom_offset = 0
    if (this.conformance_window_flag) {
      this.conf_win_left_offset = bs.ue_v()
      this.conf_win_right_offset = bs.ue_v()
      this.conf_win_top_offset = bs.ue_v()
      this.conf_win_bottom_offset = bs.ue_v()
    }
    this.bit_depth_luma_minus8 = bs.ue_v()
    this.bit_depth_chroma_minus8 = bs.ue_v()

    const subW = this.chroma_format_idc === 1 || this.chroma_format_idc === 2 ? 2 : 1
    const subH = this.chroma_format_idc === 1 ? 2 : 1
    this.picWidth = this.pic_width_in_luma_samples
    this.picHeight = this.pic_height_in_luma_samples
    this.cropRect = {
      x: subW * this.conf_win_left_offset,
      y: subH * this.conf_win_top_offset,
      width: this.picWidth - subW * (this.conf_win_left_offset + this.conf_win_right_offset),
      height: this.picHeight - subH * (this.conf_win_top_offset + this.conf_win_bottom_offset)
    }
    this.success = true
  }

  get stream() {
    return this.bitstream.stream
  }

  // ISO/IEC 14496-15 §E.1: hvc1.<profile>.<compat>.<tier+level>.<≤6 constraint bytes>
  get MIME(): string {
    const profSpace = ['', 'A', 'B', 'C'][this.general_profile_space] ?? ''
    const profile = `${profSpace}${this.general_profile_idc}`
    const compat = reverseBits32(this.general_profile_compatibility_flags)
      .toString(16)
      .toUpperCase()
    const tier = this.general_tier_flag ? 'H' : 'L'
    const tierLevel = `${tier}${this.general_level_idc}`
    const cb = [
      (this.general_constraint_indicator_flags_high >>> 16) & 0xff,
      (this.general_constraint_indicator_flags_high >>> 8) & 0xff,
      this.general_constraint_indicator_flags_high & 0xff,
      (this.general_constraint_indicator_flags_low >>> 16) & 0xff,
      (this.general_constraint_indicator_flags_low >>> 8) & 0xff,
      this.general_constraint_indicator_flags_low & 0xff
    ]
    let lastNonZero = -1
    for (let i = 0; i < cb.length; i++) if (cb[i] !== 0) lastNonZero = i
    const constraintParts =
      lastNonZero < 0 ? ['B0'] : cb.slice(0, lastNonZero + 1).map((b) => byte2hex(b).toUpperCase())
    return ['hvc1', profile, compat, tierLevel, ...constraintParts].join('.')
  }
}
