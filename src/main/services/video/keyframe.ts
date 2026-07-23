export type NalKind = 'keyframe' | 'params' | 'delta'

type Codec = 'h264' | 'h265' | 'vp9' | 'av1'

function classifyByte(headerByte: number, hevc: boolean): NalKind | null {
  if (hevc) {
    const t = (headerByte >> 1) & 0x3f
    if (t >= 16 && t <= 23) return 'keyframe'
    if (t >= 32 && t <= 34) return 'params'
    if (t <= 31) return 'delta'
    return null
  }
  const t = headerByte & 0x1f
  if (t === 5) return 'keyframe'
  if (t === 7 || t === 8) return 'params'
  if (t === 1) return 'delta'
  return null
}

// Classifies an H.264/H.265 access unit as keyframe, params-only, or delta.
export function classifyNal(frame: Buffer, codec: Codec, lengthPrefixed: boolean): NalKind {
  if (codec !== 'h264' && codec !== 'h265') return 'keyframe'
  const hevc = codec === 'h265'
  const rank = (k: NalKind): number => (k === 'keyframe' ? 2 : k === 'params' ? 1 : 0)
  let seen: NalKind | null = null
  const note = (headerByte: number): void => {
    const k = classifyByte(headerByte, hevc)
    if (k && (seen === null || rank(k) > rank(seen))) seen = k
  }

  if (!lengthPrefixed) {
    for (let i = 0; i + 3 < frame.length; i++) {
      if (frame[i] === 0 && frame[i + 1] === 0) {
        if (frame[i + 2] === 1) {
          note(frame[i + 3])
          i += 2
        } else if (frame[i + 2] === 0 && frame[i + 3] === 1 && i + 4 < frame.length) {
          note(frame[i + 4])
          i += 3
        }
      }
    }
  } else {
    let off = 0
    while (off + 4 <= frame.length) {
      const len = frame.readUInt32BE(off)
      off += 4
      if (len <= 0 || off + len > frame.length) break
      note(frame[off])
      off += len
    }
  }

  return seen ?? 'delta'
}
