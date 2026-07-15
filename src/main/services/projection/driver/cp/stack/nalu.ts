/**
 * nalu — convert CarPlay's length-prefixed H.264/H.265 into Annex-B byte-stream.
 *
 * CarPlay screen frames carry NAL units each prefixed with a 4-byte big-endian
 * length (AVCC/HVCC style), and the codec config arrives as an avcC/hvcC atom.
 * gst-host's pipeline wants Annex-B (00 00 00 01 start codes), so both are
 * rewritten here. No frame data is decoded, only the framing is rewritten.
 */

const START_CODE = Buffer.from([0x00, 0x00, 0x00, 0x01])

/** Rewrite a run of length-prefixed NAL units to Annex-B. `lengthSize` is 1..4. */
export function avccFrameToAnnexB(frame: Buffer, lengthSize = 4): Buffer {
  const parts: Buffer[] = []
  let off = 0
  while (off + lengthSize <= frame.length) {
    let n = 0
    for (let i = 0; i < lengthSize; i++) n = n * 256 + frame[off + i]
    off += lengthSize
    if (n <= 0 || off + n > frame.length) break
    parts.push(START_CODE, frame.subarray(off, off + n))
    off += n
  }
  return Buffer.concat(parts)
}

/** Extract the parameter-set NALUs from an avcC atom as Annex-B. */
function avcCToAnnexB(a: Buffer): Buffer {
  const parts: Buffer[] = []
  let off = 5 // version, profile, compat, level, lengthSizeMinusOne
  const numSps = a[off++] & 0x1f
  for (let i = 0; i < numSps && off + 2 <= a.length; i++) {
    const len = a.readUInt16BE(off)
    off += 2
    parts.push(START_CODE, a.subarray(off, off + len))
    off += len
  }
  const numPps = a[off++]
  for (let i = 0; i < numPps && off + 2 <= a.length; i++) {
    const len = a.readUInt16BE(off)
    off += 2
    parts.push(START_CODE, a.subarray(off, off + len))
    off += len
  }
  return Buffer.concat(parts)
}

/** Extract the VPS/SPS/PPS NALUs from an hvcC atom as Annex-B. */
function hvcCToAnnexB(a: Buffer): Buffer {
  const parts: Buffer[] = []
  let off = 22 // fixed profile/level block before the array count
  const numArrays = a[off++]
  for (let arr = 0; arr < numArrays && off + 3 <= a.length; arr++) {
    off++ // array_completeness + NAL_unit_type
    const numNalus = a.readUInt16BE(off)
    off += 2
    for (let i = 0; i < numNalus && off + 2 <= a.length; i++) {
      const len = a.readUInt16BE(off)
      off += 2
      parts.push(START_CODE, a.subarray(off, off + len))
      off += len
    }
  }
  return Buffer.concat(parts)
}

/** An avcC atom starts [1][profile][compat][level][fc|len][e0|numSPS][spsLen][SPS…]; the SPS NAL type is 7. */
function looksLikeAvcC(a: Buffer): boolean {
  if (a.length < 9) return false
  if ((a[5] & 0x1f) < 1) return false // numOfSPS
  const spsLen = a.readUInt16BE(6)
  if (8 + spsLen > a.length) return false
  return (a[8] & 0x1f) === 7 // H.264 SPS NAL unit type
}

/**
 * Convert a VideoConfig payload into Annex-B parameter sets, detecting the codec
 * from the atom. The payload is either a bare avcC/hvcC record, a direct box
 * ([size]['avcC'|'hvcC'][record]), or an 'hvc1'/'avc1' sample entry with the
 * avcC/hvcC box nested inside (H.265 arrives this way). The config record starts
 * right after the avcC/hvcC fourcc wherever it sits.
 */
export function configToAnnexB(payload: Buffer): { codec: 'h264' | 'h265'; annexB: Buffer } {
  for (let i = 4; i + 4 <= payload.length; i++) {
    const cc = payload.toString('ascii', i, i + 4)
    if (cc === 'hvcC') return { codec: 'h265', annexB: hvcCToAnnexB(payload.subarray(i + 4)) }
    if (cc === 'avcC') return { codec: 'h264', annexB: avcCToAnnexB(payload.subarray(i + 4)) }
  }
  // Bare atom with no fourcc (some phones send avcC as raw bytes).
  if (looksLikeAvcC(payload)) return { codec: 'h264', annexB: avcCToAnnexB(payload) }
  return { codec: 'h265', annexB: hvcCToAnnexB(payload) }
}
