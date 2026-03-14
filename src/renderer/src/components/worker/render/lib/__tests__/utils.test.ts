import { getDecoderConfig, getNaluFromStream, isKeyFrame, NaluTypes } from '../utils'

const annexB = new Uint8Array([
  0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x1f, 0xe9, 0x01, 0x40, 0x7b, 0x20, 0x00, 0x00, 0x00,
  0x01, 0x65, 0x88, 0x84, 0x21
])

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
})
