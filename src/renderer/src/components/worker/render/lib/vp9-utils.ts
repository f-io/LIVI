// VP9 bitstream utilities — parses the uncompressed keyframe header
// (VP9 Bitstream Spec §6.2) so the renderer can configure WebCodecs.
//
// VP9 has no NAL units. Each frame is self-contained; a keyframe carries
// profile/bitdepth/dimensions in the first ~10 bytes of its uncompressed
// header. Inter frames share the same opening 3 bits (frame_marker +
// profile) and an early frame_type bit, which is enough for keyframe
// detection.

import { Bitstream } from './h264-utils'

export const VP9_FRAME_MARKER = 0b10 // 2 bits, must be present in every frame
export const VP9_SYNC_CODE = [0x49, 0x83, 0x42] as const

export enum Vp9FrameType {
  KEY_FRAME = 0,
  NON_KEY_FRAME = 1
}

// VP9 colour spaces (sequence header §6.2.2)
export enum Vp9ColorSpace {
  UNKNOWN = 0,
  BT_601 = 1,
  BT_709 = 2,
  SMPTE_170 = 3,
  SMPTE_240 = 4,
  BT_2020 = 5,
  RESERVED_2 = 6,
  SRGB = 7
}

function readProfile(bs: Bitstream): number {
  const lo = bs.u_1()
  const hi = bs.u_1()
  const p = (hi << 1) | lo
  if (p === 3) bs.u_1() // reserved_zero
  return p
}

// Detection — only consumes the bits needed to determine frame_type
export function isVp9KeyFrame(frame: Uint8Array): boolean {
  if (frame.length < 1) return false
  try {
    const bs = new Bitstream(frame)
    const marker = bs.u(2)
    if (marker !== VP9_FRAME_MARKER) return false
    const profile = readProfile(bs)
    const showExisting = bs.u_1()
    if (showExisting) return false
    const frameType = bs.u_1()
    void profile
    return frameType === Vp9FrameType.KEY_FRAME
  } catch {
    return false
  }
}

// Picks the lowest VP9 level that fits a given resolution (and optional fps).
const VP9_LEVELS: Array<{ id: number; picture: number; rate: number }> = [
  { id: 10, picture: 36864, rate: 829440 },
  { id: 11, picture: 73728, rate: 2764800 },
  { id: 20, picture: 122880, rate: 4608000 },
  { id: 21, picture: 245760, rate: 9216000 },
  { id: 30, picture: 552960, rate: 20736000 },
  { id: 31, picture: 983040, rate: 36864000 },
  { id: 40, picture: 2228224, rate: 83558400 },
  { id: 41, picture: 2228224, rate: 160432128 },
  { id: 50, picture: 8912896, rate: 311951360 },
  { id: 51, picture: 8912896, rate: 588251136 },
  { id: 52, picture: 8912896, rate: 1176502272 },
  { id: 60, picture: 35651584, rate: 1176502272 },
  { id: 61, picture: 35651584, rate: 2353004544 },
  { id: 62, picture: 35651584, rate: 4706009088 }
]

export function vp9Level(width: number, height: number, fps = 30): number {
  const samples = width * height
  const rate = samples * fps
  for (const l of VP9_LEVELS) {
    if (samples <= l.picture && rate <= l.rate) return l.id
  }
  return 62
}

function pad2(n: number): string {
  const s = n.toString(10)
  return s.length < 2 ? `0${s}` : s
}

export class Vp9KeyframeHeader {
  bitstream: Bitstream

  profile: number
  show_existing_frame: 0 | 1
  frame_type: Vp9FrameType
  show_frame: 0 | 1
  error_resilient_mode: 0 | 1

  bit_depth: 8 | 10 | 12
  color_space: Vp9ColorSpace
  color_range: 0 | 1
  subsampling_x: 0 | 1
  subsampling_y: 0 | 1

  frame_width: number
  frame_height: number
  render_width: number
  render_height: number

  success: boolean

  constructor(frame: Uint8Array) {
    const bs = new Bitstream(frame)
    this.bitstream = bs

    const marker = bs.u(2)
    if (marker !== VP9_FRAME_MARKER) throw new Error('VP9 error: bad frame_marker')

    this.profile = readProfile(bs)
    this.show_existing_frame = bs.u_1() as 0 | 1
    if (this.show_existing_frame)
      throw new Error('VP9 error: show_existing_frame is not a keyframe')

    this.frame_type = bs.u_1() as Vp9FrameType
    if (this.frame_type !== Vp9FrameType.KEY_FRAME) {
      throw new Error('VP9 error: not a keyframe')
    }
    this.show_frame = bs.u_1() as 0 | 1
    this.error_resilient_mode = bs.u_1() as 0 | 1

    // frame_sync_code()
    const s1 = bs.u_8()
    const s2 = bs.u_8()
    const s3 = bs.u_8()
    if (s1 !== VP9_SYNC_CODE[0] || s2 !== VP9_SYNC_CODE[1] || s3 !== VP9_SYNC_CODE[2]) {
      throw new Error('VP9 error: bad frame_sync_code')
    }

    // color_config()
    if (this.profile >= 2) {
      const tenOrTwelve = bs.u_1()
      this.bit_depth = tenOrTwelve ? 12 : 10
    } else {
      this.bit_depth = 8
    }
    this.color_space = bs.u(3) as Vp9ColorSpace
    if (this.color_space !== Vp9ColorSpace.SRGB) {
      this.color_range = bs.u_1() as 0 | 1
      if (this.profile === 1 || this.profile === 3) {
        this.subsampling_x = bs.u_1() as 0 | 1
        this.subsampling_y = bs.u_1() as 0 | 1
        bs.u_1() // reserved_zero
      } else {
        this.subsampling_x = 1
        this.subsampling_y = 1
      }
    } else {
      this.color_range = 1
      if (this.profile === 1 || this.profile === 3) {
        bs.u_1() // reserved_zero
        this.subsampling_x = 0
        this.subsampling_y = 0
      } else {
        // profile 0/2 + sRGB is not allowed; fall back gracefully
        this.subsampling_x = 0
        this.subsampling_y = 0
      }
    }

    // frame_size()
    const fwm1 = bs.u(16)
    const fhm1 = bs.u(16)
    this.frame_width = fwm1 + 1
    this.frame_height = fhm1 + 1
    const renderDifferent = bs.u_1()
    if (renderDifferent) {
      this.render_width = bs.u(16) + 1
      this.render_height = bs.u(16) + 1
    } else {
      this.render_width = this.frame_width
      this.render_height = this.frame_height
    }

    this.success = true
  }

  get stream() {
    return this.bitstream.stream
  }

  // RFC 6381 / WebCodecs short form: vp09.<profile>.<level>.<bit_depth>.
  // Optional fields (chroma, color, transfer, matrix, full_range) are
  // omitted — Chromium accepts the truncated codec string for configure().
  mime(level?: number): string {
    const lvl = level ?? vp9Level(this.frame_width, this.frame_height)
    return ['vp09', pad2(this.profile), pad2(lvl), pad2(this.bit_depth)].join('.')
  }
}

export function getVp9DecoderConfig(
  frame: Uint8Array,
  fps?: number
): { codec: string; codedWidth: number; codedHeight: number } | null {
  try {
    const hdr = new Vp9KeyframeHeader(frame)
    return {
      codec: hdr.mime(vp9Level(hdr.frame_width, hdr.frame_height, fps ?? 30)),
      codedWidth: hdr.frame_width,
      codedHeight: hdr.frame_height
    }
  } catch (e) {
    console.warn('[vp9-utils] getVp9DecoderConfig failed:', e)
    return null
  }
}
