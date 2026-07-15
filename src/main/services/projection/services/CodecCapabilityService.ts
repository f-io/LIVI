import { probeGstCodecs } from '../../video/GstVideo'

type Caps = Record<string, { hw?: unknown; sw?: unknown } | undefined>

export type CodecKind = 'hevc' | 'vp9' | 'av1'

export class CodecCapabilityService {
  private hevcSupported = false
  private vp9Supported = false
  private av1Supported = false
  private lastCodecCaps: Caps | null = null

  constructor(private readonly onSupportChange: (codec: CodecKind, supported: boolean) => void) {}

  get hevc(): boolean {
    return this.hevcSupported
  }

  get vp9(): boolean {
    return this.vp9Supported
  }

  get av1(): boolean {
    return this.av1Supported
  }

  applyCodecCapabilities(payload: unknown): void {
    if (!payload || typeof payload !== 'object') return
    this.lastCodecCaps = payload as Caps
    this.recompute()
  }

  // Advertise the codecs the bundled GStreamer can decode. Optional codecs are
  // offered only when a HW decoder exists, h264 is the always-on AA baseline
  applyGstCodecCaps(): void {
    const p = probeGstCodecs()
    const hwCap = (s: { hw: boolean }): { hw?: unknown; sw?: unknown } | undefined =>
      s.hw ? { hw: true, sw: true } : undefined
    const h265Cap = p.h265.hw || (p.h265.sw && !p.h264.hw) ? { hw: true, sw: true } : undefined
    this.lastCodecCaps = {
      h264: { hw: true, sw: true },
      h265: h265Cap,
      vp9: hwCap(p.vp9),
      av1: hwCap(p.av1)
    }
    console.log(
      `[CodecCapability] GStreamer codecs: ` +
        `h264(hw=${p.h264.hw} sw=${p.h264.sw}) ` +
        `h265(hw=${p.h265.hw} sw=${p.h265.sw}) ` +
        `vp9(hw=${p.vp9.hw} sw=${p.vp9.sw}) ` +
        `av1(hw=${p.av1.hw} sw=${p.av1.sw})`
    )
    this.recompute()
  }

  private recompute(): void {
    const caps = this.lastCodecCaps
    if (!caps) return
    // applyGstCodecCaps already drops optional codecs without a HW decoder to
    // undefined, so a present entry means the codec is advertised
    const isSupported = (c: { hw?: unknown; sw?: unknown } | undefined): boolean => Boolean(c)

    const hevc = isSupported(caps.h265)
    const vp9 = isSupported(caps.vp9)
    const av1 = isSupported(caps.av1)

    if (this.hevcSupported !== hevc) {
      this.hevcSupported = hevc
      console.log(`[CodecCapability] hevc support: ${hevc}`)
      this.onSupportChange('hevc', hevc)
    }
    if (this.vp9Supported !== vp9) {
      this.vp9Supported = vp9
      console.log(`[CodecCapability] vp9 support: ${vp9}`)
      this.onSupportChange('vp9', vp9)
    }
    if (this.av1Supported !== av1) {
      this.av1Supported = av1
      console.log(`[CodecCapability] av1 support: ${av1}`)
      this.onSupportChange('av1', av1)
    }
  }
}
