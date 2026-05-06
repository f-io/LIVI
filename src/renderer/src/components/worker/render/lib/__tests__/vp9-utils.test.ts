import {
  getVp9DecoderConfig,
  isVp9KeyFrame,
  Vp9ColorSpace,
  Vp9KeyframeHeader,
  vp9Level
} from '../vp9-utils'

// Minimal bit-builder so tests can describe VP9 frame headers in spec terms
// instead of hand-packed bytes.
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

interface KeyframeOpts {
  profile?: 0 | 1 | 2 | 3
  showFrame?: 0 | 1
  errorResilient?: 0 | 1
  bitDepth?: 8 | 10 | 12
  colorSpace?: Vp9ColorSpace
  colorRange?: 0 | 1
  subsamplingX?: 0 | 1
  subsamplingY?: 0 | 1
  width: number
  height: number
}

function buildVp9Keyframe(opts: KeyframeOpts): Uint8Array {
  const profile = opts.profile ?? 0
  const b = new BitBuilder()
  b.put(0b10, 2) // frame_marker
  b.put(profile & 1, 1) // profile_low
  b.put((profile >> 1) & 1, 1) // profile_high
  if (profile === 3) b.put(0, 1)
  b.put(0, 1) // show_existing_frame
  b.put(0, 1) // frame_type = KEY_FRAME
  b.put(opts.showFrame ?? 1, 1)
  b.put(opts.errorResilient ?? 0, 1)
  // sync_code
  b.put(0x49, 8)
  b.put(0x83, 8)
  b.put(0x42, 8)
  // color_config
  if (profile >= 2) {
    const bd = opts.bitDepth ?? 8
    if (bd === 8) throw new Error('profile >=2 requires 10 or 12 bit')
    b.put(bd === 12 ? 1 : 0, 1)
  }
  const colorSpace = opts.colorSpace ?? Vp9ColorSpace.BT_601
  b.put(colorSpace, 3)
  if (colorSpace !== Vp9ColorSpace.SRGB) {
    b.put(opts.colorRange ?? 0, 1)
    if (profile === 1 || profile === 3) {
      b.put(opts.subsamplingX ?? 1, 1)
      b.put(opts.subsamplingY ?? 1, 1)
      b.put(0, 1) // reserved_zero
    }
  } else {
    if (profile === 1 || profile === 3) b.put(0, 1) // reserved_zero
  }
  // frame_size
  b.put(opts.width - 1, 16)
  b.put(opts.height - 1, 16)
  b.put(0, 1) // render_and_frame_size_different
  return b.toBytes()
}

describe('vp9-utils', () => {
  test('isVp9KeyFrame detects keyframe', () => {
    const frame = buildVp9Keyframe({ width: 1920, height: 1080 })
    expect(isVp9KeyFrame(frame)).toBe(true)
  })

  test('isVp9KeyFrame returns false on inter-frame', () => {
    // frame_marker=10, profile=0, show_existing=0, frame_type=1 (NON_KEY)
    // bits: 10 0 0 0 1 → first 6 bits set; rest don't matter for our quick check
    const inter = new Uint8Array([0b10000100])
    expect(isVp9KeyFrame(inter)).toBe(false)
  })

  test('isVp9KeyFrame rejects bad frame_marker', () => {
    const bad = new Uint8Array([0b00000000])
    expect(isVp9KeyFrame(bad)).toBe(false)
  })

  test('isVp9KeyFrame returns false on show_existing_frame', () => {
    // marker(2)=10, profile_lo=0, profile_hi=0, show_existing=1
    // bits: 10 0 0 1 ... → first 5 bits = 10001
    const showExisting = new Uint8Array([0b10001000])
    expect(isVp9KeyFrame(showExisting)).toBe(false)
  })

  test('Vp9KeyframeHeader parses 1920x1080 profile 0', () => {
    const frame = buildVp9Keyframe({ width: 1920, height: 1080 })
    const hdr = new Vp9KeyframeHeader(frame)
    expect(hdr.success).toBe(true)
    expect(hdr.profile).toBe(0)
    expect(hdr.bit_depth).toBe(8)
    expect(hdr.frame_width).toBe(1920)
    expect(hdr.frame_height).toBe(1080)
    expect(hdr.color_space).toBe(Vp9ColorSpace.BT_601)
  })

  test('Vp9KeyframeHeader parses profile 2 with 10-bit', () => {
    const frame = buildVp9Keyframe({
      profile: 2,
      bitDepth: 10,
      width: 3840,
      height: 2160
    })
    const hdr = new Vp9KeyframeHeader(frame)
    expect(hdr.profile).toBe(2)
    expect(hdr.bit_depth).toBe(10)
    expect(hdr.frame_width).toBe(3840)
    expect(hdr.frame_height).toBe(2160)
  })

  test('Vp9KeyframeHeader rejects bad sync code', () => {
    const bad = new Uint8Array([0x82, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00])
    expect(() => new Vp9KeyframeHeader(bad)).toThrow(/sync_code/)
  })

  test('vp9Level picks the lowest level that fits', () => {
    expect(vp9Level(1920, 1080, 30)).toBe(40)
    expect(vp9Level(1920, 1080, 60)).toBe(41)
    expect(vp9Level(3840, 2160, 30)).toBe(50)
    expect(vp9Level(3840, 2160, 60)).toBe(51)
    expect(vp9Level(640, 480, 30)).toBe(30)
  })

  test('Vp9KeyframeHeader.mime emits vp09.<profile>.<level>.<bitDepth>', () => {
    const frame = buildVp9Keyframe({ width: 1920, height: 1080 })
    const hdr = new Vp9KeyframeHeader(frame)
    expect(hdr.mime()).toBe('vp09.00.40.08')
  })

  test('getVp9DecoderConfig returns config for keyframe', () => {
    const frame = buildVp9Keyframe({ width: 1280, height: 720 })
    const cfg = getVp9DecoderConfig(frame, 30)
    expect(cfg).not.toBeNull()
    expect(cfg?.codec).toMatch(/^vp09\./)
    expect(cfg?.codedWidth).toBe(1280)
    expect(cfg?.codedHeight).toBe(720)
  })

  test('getVp9DecoderConfig returns null on garbage', () => {
    expect(getVp9DecoderConfig(new Uint8Array([0, 0, 0, 0]))).toBeNull()
  })
})
