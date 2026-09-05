import type { Mock } from 'vitest'
import { probeGstCodecs } from '../../../video/GstVideo'
import { CodecCapabilityService, type CodecKind, offersHevc } from '../CodecCapabilityService'

vi.mock('../../../video/GstVideo', () => ({
  probeGstCodecs: vi.fn()
}))

type Probe = ReturnType<typeof probeGstCodecs>

function mkProbe(over: Partial<Probe> = {}): Probe {
  return {
    h264: { hw: true, sw: true },
    h265: { hw: false, sw: false },
    vp9: { hw: false, sw: false },
    av1: { hw: false, sw: false },
    ...over
  } as Probe
}

describe('CodecCapabilityService', () => {
  let logSpy: ReturnType<typeof vi.spyOn>

  beforeEach(() => {
    logSpy = vi.spyOn(console, 'log').mockImplementation(() => undefined)
  })

  afterEach(() => {
    logSpy.mockRestore()
    vi.clearAllMocks()
  })

  test('starts with every codec unsupported', () => {
    const svc = new CodecCapabilityService(vi.fn())

    expect(svc.hevc).toBe(false)
    expect(svc.vp9).toBe(false)
    expect(svc.av1).toBe(false)
  })

  test('applyCodecCapabilities ignores null and non-object payloads', () => {
    const onChange = vi.fn()
    const svc = new CodecCapabilityService(onChange)

    svc.applyCodecCapabilities(null)
    svc.applyCodecCapabilities('h265')

    expect(onChange).not.toHaveBeenCalled()
    expect(svc.hevc).toBe(false)
  })

  test('applyCodecCapabilities flips supported codecs on and notifies once per change', () => {
    const changes: Array<[CodecKind, boolean]> = []
    const svc = new CodecCapabilityService((codec, supported) => changes.push([codec, supported]))

    svc.applyCodecCapabilities({ h265: { hw: true }, vp9: { sw: true }, av1: { hw: true } })

    expect(svc.hevc).toBe(true)
    expect(svc.vp9).toBe(true)
    expect(svc.av1).toBe(true)
    expect(changes).toEqual([
      ['hevc', true],
      ['vp9', true],
      ['av1', true]
    ])

    svc.applyCodecCapabilities({ h265: { hw: true }, vp9: { sw: true }, av1: { hw: true } })
    expect(changes).toHaveLength(3)

    svc.applyCodecCapabilities({})
    expect(svc.hevc).toBe(false)
    expect(svc.vp9).toBe(false)
    expect(svc.av1).toBe(false)
    expect(changes).toHaveLength(6)
  })

  test('applyGstCodecCaps advertises hevc when a HW decoder exists', () => {
    const onChange = vi.fn()
    const svc = new CodecCapabilityService(onChange)
    ;(probeGstCodecs as Mock).mockReturnValue(
      mkProbe({
        h265: { hw: true, sw: true },
        vp9: { hw: true, sw: true },
        av1: { hw: true, sw: false }
      })
    )

    svc.applyGstCodecCaps()

    expect(svc.hevc).toBe(true)
    expect(svc.vp9).toBe(true)
    expect(svc.av1).toBe(true)
  })

  test('applyGstCodecCaps advertises sw hevc only when h264 has no HW decoder', () => {
    const svc = new CodecCapabilityService(vi.fn())
    ;(probeGstCodecs as Mock).mockReturnValue(
      mkProbe({
        h264: { hw: false, sw: true },
        h265: { hw: false, sw: true }
      })
    )

    svc.applyGstCodecCaps()

    expect(svc.hevc).toBe(true)
    expect(svc.vp9).toBe(false)
    expect(svc.av1).toBe(false)
  })

  test('sw hevc yields to a hardware h264, except on macOS', () => {
    const swOnly = { h264: { hw: true }, h265: { hw: false, sw: true } }
    expect(offersHevc(swOnly, 'linux')).toBe(false)
    expect(offersHevc(swOnly, 'darwin')).toBe(true)
    expect(offersHevc({ h264: { hw: false }, h265: { hw: false, sw: true } }, 'linux')).toBe(true)
    expect(offersHevc({ h264: { hw: true }, h265: { hw: false, sw: false } }, 'darwin')).toBe(false)
  })

  test('applyGstCodecCaps drops every optional codec without any decoder', () => {
    const onChange = vi.fn()
    const svc = new CodecCapabilityService(onChange)
    ;(probeGstCodecs as Mock).mockReturnValue(mkProbe())

    svc.applyGstCodecCaps()

    expect(svc.hevc).toBe(false)
    expect(svc.vp9).toBe(false)
    expect(svc.av1).toBe(false)
    expect(onChange).not.toHaveBeenCalled()
  })
})
