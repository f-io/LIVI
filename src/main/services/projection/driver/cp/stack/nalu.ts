/**
 * nalu — read CarPlay's VideoConfig atom.
 *
 * The config arrives as an avcC/hvcC record (sometimes wrapped in a box or an
 * hvc1/avc1 sample entry). It is handed to the pipeline as codec_data so the parser
 * reads the parameter sets; frames stay length-prefixed and are never decoded or
 * rewritten here.
 */

/** An avcC atom starts [1][profile][compat][level][fc|len][e0|numSPS][spsLen][SPS…]; the SPS NAL type is 7. */
function looksLikeAvcC(a: Buffer): boolean {
  if (a.length < 9) return false
  if ((a[5] & 0x1f) < 1) return false // numOfSPS
  const spsLen = a.readUInt16BE(6)
  if (8 + spsLen > a.length) return false
  return (a[8] & 0x1f) === 7 // H.264 SPS NAL unit type
}

/**
 * Locate the avcC/hvcC record in a VideoConfig payload and return it verbatim as the
 * decoder's codec_data, with the detected codec (the phone may pick H.264 even when H.265
 * is offered). The payload is either a bare record, a direct box ([size]['avcC'|'hvcC']
 * [record]), or an 'hvc1'/'avc1' sample entry with the box nested inside; the record starts
 * right after the avcC/hvcC fourcc wherever it sits.
 */
export function configToCodecData(payload: Buffer): { codec: 'h264' | 'h265'; codecData: Buffer } {
  for (let i = 4; i + 4 <= payload.length; i++) {
    const cc = payload.toString('ascii', i, i + 4)
    if (cc === 'hvcC') return { codec: 'h265', codecData: payload.subarray(i + 4) }
    if (cc === 'avcC') return { codec: 'h264', codecData: payload.subarray(i + 4) }
  }
  if (looksLikeAvcC(payload)) return { codec: 'h264', codecData: payload }
  return { codec: 'h265', codecData: payload }
}
