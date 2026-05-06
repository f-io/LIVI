// AV1 bitstream utilities — parses OBU stream + sequence_header_obu so the
// renderer can configure WebCodecs.
//
// AV1 frames are delivered as a sequence of OBUs (Open Bitstream Units).
// Each OBU has a 1- or 2-byte header, optional leb128 size, then payload.
// Config info lives in SEQUENCE_HEADER (profile, level, tier, bit_depth);
// frame data lives in FRAME or FRAME_HEADER + TILE_GROUP. Keyframes are
// FRAME/FRAME_HEADER OBUs whose first uncompressed bit reveals frame_type.
//
// AV1 Bitstream Spec §5.3, §5.5.

import { Bitstream } from './h264-utils'

export enum Av1ObuType {
  RESERVED_0 = 0,
  SEQUENCE_HEADER = 1,
  TEMPORAL_DELIMITER = 2,
  FRAME_HEADER = 3,
  TILE_GROUP = 4,
  METADATA = 5,
  FRAME = 6,
  REDUNDANT_FRAME_HEADER = 7,
  TILE_LIST = 8,
  PADDING = 15
}

export enum Av1FrameType {
  KEY_FRAME = 0,
  INTER_FRAME = 1,
  INTRA_ONLY_FRAME = 2,
  SWITCH_FRAME = 3
}

export interface Av1Obu {
  type: Av1ObuType
  extensionFlag: boolean
  hasSizeField: boolean
  temporalId: number
  spatialId: number
  payload: Uint8Array
  rawStart: number
  rawEnd: number
}

// leb128 reader — returns { value, length } where length is the number of
// bytes consumed
export function readLeb128(
  buffer: Uint8Array,
  offset: number
): { value: number; length: number } | null {
  let value = 0
  let length = 0
  for (let i = 0; i < 8; i++) {
    if (offset + i >= buffer.length) return null
    const b = buffer[offset + i]!
    value |= (b & 0x7f) << (i * 7)
    length++
    if ((b & 0x80) === 0) return { value: value >>> 0, length }
  }
  return { value: value >>> 0, length }
}

// Walk an AV1 packet and yield every OBU it contains
export function* parseAv1Obus(buffer: Uint8Array): Generator<Av1Obu> {
  let p = 0
  while (p < buffer.length) {
    const b0 = buffer[p]!
    if ((b0 & 0x80) !== 0) return // forbidden_bit set — stop
    const type = ((b0 >> 3) & 0x0f) as Av1ObuType
    const extensionFlag = (b0 & 0x04) !== 0
    const hasSizeField = (b0 & 0x02) !== 0
    let header = 1
    let temporalId = 0
    let spatialId = 0
    if (extensionFlag) {
      if (p + 1 >= buffer.length) return
      const b1 = buffer[p + 1]!
      temporalId = (b1 >> 5) & 0x07
      spatialId = (b1 >> 3) & 0x03
      header = 2
    }
    let payloadOffset = p + header
    let payloadLen: number
    if (hasSizeField) {
      const leb = readLeb128(buffer, payloadOffset)
      if (!leb) return
      payloadOffset += leb.length
      payloadLen = leb.value
    } else {
      payloadLen = buffer.length - payloadOffset
    }
    if (payloadOffset + payloadLen > buffer.length) {
      // Truncated — surface what we have so callers can still inspect headers.
      payloadLen = Math.max(0, buffer.length - payloadOffset)
    }
    const payload = buffer.subarray(payloadOffset, payloadOffset + payloadLen)
    yield {
      type,
      extensionFlag,
      hasSizeField,
      temporalId,
      spatialId,
      payload,
      rawStart: p,
      rawEnd: payloadOffset + payloadLen
    }
    p = payloadOffset + payloadLen
  }
}

export function findAv1Obu(buffer: Uint8Array, type: Av1ObuType): Av1Obu | null {
  for (const obu of parseAv1Obus(buffer)) {
    if (obu.type === type) return obu
  }
  return null
}

// AV1 frame_type lives at a known bit-position inside FRAME / FRAME_HEADER
// payloads — preceded by show_existing_frame (1 bit) and an optional
// frame_to_show_map_idx (3 bits) skip path. We only need to determine
// frame_type === KEY_FRAME, no full uncompressed_header parse.
function readAv1FrameType(payload: Uint8Array): Av1FrameType | null {
  if (payload.length < 1) return null
  const bs = new Bitstream(payload)
  const showExisting = bs.u_1()
  if (showExisting) return null // not a keyframe; caller should treat as non-key
  const frameType = bs.u(2)
  return frameType as Av1FrameType
}

export function isAv1KeyFrame(buffer: Uint8Array): boolean {
  for (const obu of parseAv1Obus(buffer)) {
    if (obu.type === Av1ObuType.FRAME || obu.type === Av1ObuType.FRAME_HEADER) {
      const ft = readAv1FrameType(obu.payload)
      if (ft === Av1FrameType.KEY_FRAME) return true
    }
  }
  return false
}

export function containsAv1SequenceHeader(buffer: Uint8Array): boolean {
  for (const obu of parseAv1Obus(buffer)) {
    if (obu.type === Av1ObuType.SEQUENCE_HEADER) return true
  }
  return false
}

function pad2(n: number): string {
  const s = n.toString(10)
  return s.length < 2 ? `0${s}` : s
}

export class Av1SequenceHeader {
  bitstream: Bitstream

  seq_profile: 0 | 1 | 2
  still_picture: 0 | 1
  reduced_still_picture_header: 0 | 1
  seq_level_idx_0: number
  seq_tier_0: 0 | 1
  high_bitdepth: 0 | 1
  twelve_bit: 0 | 1
  bit_depth: 8 | 10 | 12
  monochrome: 0 | 1
  max_frame_width: number
  max_frame_height: number

  success: boolean

  constructor(payload: Uint8Array) {
    const bs = new Bitstream(payload)
    this.bitstream = bs

    this.seq_profile = bs.u(3) as 0 | 1 | 2
    this.still_picture = bs.u_1() as 0 | 1
    this.reduced_still_picture_header = bs.u_1() as 0 | 1

    if (this.reduced_still_picture_header) {
      this.seq_level_idx_0 = bs.u(5)
      this.seq_tier_0 = 0
    } else {
      const timingInfoPresent = bs.u_1()
      let decoderModelInfoPresent = 0
      if (timingInfoPresent) {
        // timing_info()
        bs.u(32) // num_units_in_display_tick
        bs.u(32) // time_scale
        const equalPictureInterval = bs.u_1()
        if (equalPictureInterval) this.skipUvlc(bs)
        decoderModelInfoPresent = bs.u_1()
        if (decoderModelInfoPresent) {
          // decoder_model_info(): 5 + 32 + 10 + 5 = 52 bits
          bs.u(5) // buffer_delay_length_minus_1 (we re-use as length proxy)
          const bufferDelayLengthMinus1 = 4 // spec default; we'll skip exactly 5+32+10+5
          void bufferDelayLengthMinus1
          bs.u(32)
          bs.u(10)
          bs.u(5)
        }
      }
      const initialDisplayDelayPresent = bs.u_1()
      const operatingPointsCntMinus1 = bs.u(5)
      // We only consume the values for [0]; rest of operating points are
      // skipped to keep the parser compact.
      this.seq_level_idx_0 = 0
      this.seq_tier_0 = 0
      for (let i = 0; i <= operatingPointsCntMinus1; i++) {
        bs.u(12) // operating_point_idc[i]
        const lvl = bs.u(5)
        let tier: 0 | 1 = 0
        if (lvl > 7) tier = bs.u_1() as 0 | 1
        if (i === 0) {
          this.seq_level_idx_0 = lvl
          this.seq_tier_0 = tier
        }
        if (decoderModelInfoPresent) {
          const modelPresent = bs.u_1()
          if (modelPresent) {
            // operating_parameters_info(): 2 × bufferDelayLengthMinus1+1 bits
            // We don't track the real length — best-effort skip 32+32+1.
            bs.u(32)
            bs.u(32)
            bs.u(1)
          }
        }
        if (initialDisplayDelayPresent) {
          const present = bs.u_1()
          if (present) bs.u(4)
        }
      }
    }

    const frameWidthBitsMinus1 = bs.u(4)
    const frameHeightBitsMinus1 = bs.u(4)
    const fwm1 = bs.u(frameWidthBitsMinus1 + 1)
    const fhm1 = bs.u(frameHeightBitsMinus1 + 1)
    this.max_frame_width = fwm1 + 1
    this.max_frame_height = fhm1 + 1

    if (!this.reduced_still_picture_header) {
      const frameIdNumbersPresent = bs.u_1()
      if (frameIdNumbersPresent) {
        bs.u(4) // delta_frame_id_length_minus_2
        bs.u(3) // additional_frame_id_length_minus_1
      }
    }

    bs.u_1() // use_128x128_superblock
    bs.u_1() // enable_filter_intra
    bs.u_1() // enable_intra_edge_filter

    if (!this.reduced_still_picture_header) {
      bs.u_1() // enable_interintra_compound
      bs.u_1() // enable_masked_compound
      bs.u_1() // enable_warped_motion
      bs.u_1() // enable_dual_filter
      const enableOrderHint = bs.u_1()
      if (enableOrderHint) {
        bs.u_1() // enable_jnt_comp
        bs.u_1() // enable_ref_frame_mvs
      }
      const seqChooseScreenDetectionTools = bs.u_1()
      let seqForceScreenDetectionTools = 0
      if (seqChooseScreenDetectionTools) {
        seqForceScreenDetectionTools = 2 // SELECT_SCREEN_CONTENT_TOOLS
      } else {
        seqForceScreenDetectionTools = bs.u_1()
      }
      if (seqForceScreenDetectionTools > 0) {
        const seqChooseIntegerMv = bs.u_1()
        if (!seqChooseIntegerMv) bs.u_1() // seq_force_integer_mv
      }
      if (enableOrderHint) bs.u(3) // order_hint_bits_minus_1
    }

    bs.u_1() // enable_superres
    bs.u_1() // enable_cdef
    bs.u_1() // enable_restoration

    // color_config()
    this.high_bitdepth = bs.u_1() as 0 | 1
    if (this.seq_profile === 2 && this.high_bitdepth) {
      this.twelve_bit = bs.u_1() as 0 | 1
      this.bit_depth = this.twelve_bit ? 12 : 10
    } else {
      this.twelve_bit = 0
      this.bit_depth = this.high_bitdepth ? 10 : 8
    }
    if (this.seq_profile === 1) {
      this.monochrome = 0
    } else {
      this.monochrome = bs.u_1() as 0 | 1
    }

    this.success = true
  }

  // Skip a uvlc-coded value (used after equal_picture_interval). uvlc reads
  // leading zeros then 1+leadingZeros bits.
  private skipUvlc(bs: Bitstream): void {
    let leading = 0
    while (leading < 32) {
      if (bs.u_1() === 1) break
      leading++
    }
    if (leading > 0) bs.u(leading)
  }

  get stream() {
    return this.bitstream.stream
  }

  // RFC 6381 / WebCodecs short form: av01.<profile>.<level><tier>.<bitDepth>.
  // Tier is 'M' (main) or 'H' (high).
  get MIME(): string {
    const tier = this.seq_tier_0 === 1 ? 'H' : 'M'
    const level = pad2(this.seq_level_idx_0)
    return ['av01', this.seq_profile.toString(10), `${level}${tier}`, pad2(this.bit_depth)].join(
      '.'
    )
  }
}

export function getAv1DecoderConfig(
  buffer: Uint8Array
): { codec: string; codedWidth: number; codedHeight: number } | null {
  try {
    const seq = findAv1Obu(buffer, Av1ObuType.SEQUENCE_HEADER)
    if (!seq) return null
    const hdr = new Av1SequenceHeader(seq.payload)
    return {
      codec: hdr.MIME,
      codedWidth: hdr.max_frame_width,
      codedHeight: hdr.max_frame_height
    }
  } catch (e) {
    console.warn('[av1-utils] getAv1DecoderConfig failed:', e)
    return null
  }
}
