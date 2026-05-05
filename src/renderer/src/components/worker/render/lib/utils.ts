import { Bitstream, NALUStream, SPS, type StreamType } from './h264-utils'
import { HevcNaluTypes, HevcSPS, isHevcIrapNalType, readHevcNalType } from './h265-utils'

export type VideoCodec = 'h264' | 'h265'

export enum NaluTypes {
  NDR = 1,
  IDR = 5,
  SEI = 6,
  SPS = 7,
  PPS = 8,
  AUD = 9
}

export interface GetNaluResult {
  nalu: Uint8Array
  rawNalu: Uint8Array
  type: number
}

function readNalType(nalu: Uint8Array, codec: VideoCodec): number {
  if (codec === 'h265') return (nalu[0]! >> 1) & 0x3f
  // H.264: bit layout is [forbidden_zero, nal_ref_idc(2), nal_unit_type(5)].
  const bs = new Bitstream(nalu)
  bs.seek(3)
  return bs.u(5)
}

function findNalu(
  buffer: Uint8Array,
  type: number,
  codec: VideoCodec,
  streamType: StreamType
): GetNaluResult | null {
  const stream = new NALUStream(buffer, { type: streamType })
  const minLen = codec === 'h265' ? 2 : 1
  for (const nalu of stream.nalus()) {
    if (!nalu?.nalu || nalu.nalu.length < Math.max(minLen, 4)) continue
    if (readNalType(nalu.nalu, codec) === type) {
      return {
        nalu: nalu.nalu,
        rawNalu: nalu.rawNalu!,
        type
      }
    }
  }
  return null
}

// H.264-only legacy signature, kept stable for existing callers and tests.
export function getNaluFromStream(
  buffer: Uint8Array,
  type: NaluTypes,
  streamType: StreamType = 'annexB'
): GetNaluResult | null {
  return findNalu(buffer, type, 'h264', streamType)
}

// Codec-aware SPS lookup. H.264 SPS is NAL type 7, HEVC SPS is type 33.
export function getSpsFromStream(
  buffer: Uint8Array,
  codec: VideoCodec,
  streamType: StreamType = 'annexB'
): GetNaluResult | null {
  const t = codec === 'h265' ? HevcNaluTypes.SPS : NaluTypes.SPS
  return findNalu(buffer, t, codec, streamType)
}

export function getDecoderConfig(
  data: Uint8Array,
  codec: VideoCodec = 'h264'
): { codec: string; codedWidth: number; codedHeight: number } | null {
  for (const st of ['annexB', 'packet'] as StreamType[]) {
    try {
      const res = getSpsFromStream(data, codec, st)
      if (!res) continue
      if (codec === 'h265') {
        const sps = new HevcSPS(res.nalu)
        return { codec: sps.MIME, codedWidth: sps.picWidth, codedHeight: sps.picHeight }
      }
      const sps = new SPS(res.nalu)
      return { codec: sps.MIME, codedWidth: sps.picWidth, codedHeight: sps.picHeight }
    } catch (e) {
      console.warn(`[lib/utils] getDecoderConfig (${codec}) failed for ${st}:`, e)
    }
  }
  return null
}

export function isKeyFrame(data: Uint8Array, codec: VideoCodec = 'h264'): boolean {
  if (codec === 'h265') {
    const stream = new NALUStream(data, { type: 'annexB' })
    for (const nalu of stream.nalus()) {
      if (!nalu?.nalu || nalu.nalu.length < 2) continue
      if (isHevcIrapNalType(readHevcNalType(nalu.nalu))) return true
    }
    return false
  }
  return findNalu(data, NaluTypes.IDR, 'h264', 'annexB') !== null
}

// True if the chunk contains any parameter-set NAL: H.264 needs SPS+PPS,
// HEVC additionally needs VPS
export function containsParameterSet(data: Uint8Array, codec: VideoCodec): boolean {
  const stream = new NALUStream(data, { type: 'annexB' })
  const minLen = codec === 'h265' ? 2 : 1
  for (const nalu of stream.nalus()) {
    if (!nalu?.nalu || nalu.nalu.length < Math.max(minLen, 4)) continue
    const t = readNalType(nalu.nalu, codec)
    if (codec === 'h265') {
      if (t === HevcNaluTypes.VPS || t === HevcNaluTypes.SPS || t === HevcNaluTypes.PPS) return true
    } else {
      if (t === NaluTypes.SPS || t === NaluTypes.PPS) return true
    }
  }
  return false
}

export { SPS } from './h264-utils'
export { HevcSPS } from './h265-utils'
