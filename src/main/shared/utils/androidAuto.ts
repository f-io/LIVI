export const clamp = (number: number, min: number, max: number) => {
  return Math.max(min, Math.min(number, max))
}

export function getCurrentTimeInMs() {
  return Math.round(Date.now() / 1000)
}

export type AndroidAutoResolution = {
  // Canonical 16:9 tier the phone encodes into (`videoWidth`/`videoHeight`).
  width: number
  height: number
}

const roundEven = (n: number): number => Math.max(2, Math.floor(n) & ~1)

// AR-preserved content area within an AA encoder frame.
export function aaContentArea(
  frame: { width: number; height: number },
  user: { width: number; height: number }
): { contentWidth: number; contentHeight: number } {
  const userAR = Math.max(1, user.width) / Math.max(1, user.height)
  const frameAR = Math.max(1, frame.width) / Math.max(1, frame.height)
  if (userAR <= frameAR) {
    return { contentWidth: roundEven(frame.height * userAR), contentHeight: frame.height }
  }
  return { contentWidth: frame.width, contentHeight: roundEven(frame.width / userAR) }
}

export function dongleDisplayName(name: string): string {
  return `${name} (D)`
}

// Android Auto only encodes into these canonical 16:9 frames.
const AA_TIERS: ReadonlyArray<{ w: number; h: number }> = [
  { w: 800, h: 480 },
  { w: 1280, h: 720 },
  { w: 1920, h: 1080 },
  { w: 2560, h: 1440 },
  { w: 3840, h: 2160 }
]

// Phones reject AA sessions that request a tier above 1920×1080 unless
// the HU also advertises a codec other than H.264 (HEVC / VP9 / AV1).
const H264_MAX_TIER_WIDTH = 1920

export type AAResolutionOptions = {
  h264Only?: boolean
}

const MAX_TIER_UPSCALE = 1.2

export function matchFittingAAResolution(
  userRes: { width: number; height: number },
  options?: AAResolutionOptions
): AndroidAutoResolution {
  const userW = Math.max(1, userRes.width)
  const userH = Math.max(1, userRes.height)
  const maxTierW = options?.h264Only ? H264_MAX_TIER_WIDTH : Infinity

  let chosen = AA_TIERS[0]
  for (const tier of AA_TIERS) {
    if (tier.w > maxTierW) break
    chosen = tier
    const { contentWidth, contentHeight } = aaContentArea(
      { width: tier.w, height: tier.h },
      { width: userW, height: userH }
    )
    const upscale = Math.max(userW / contentWidth, userH / contentHeight)
    if (upscale <= MAX_TIER_UPSCALE) break
  }

  return { width: chosen.w, height: chosen.h }
}

export function pixelAspectRatioE4(
  display: { width: number; height: number },
  tier: { width: number; height: number }
): number {
  const dW = Math.max(1, display.width)
  const dH = Math.max(1, display.height)
  const tW = Math.max(1, tier.width)
  const tH = Math.max(1, tier.height)
  const displayAR = dW / dH
  const tierAR = tW / tH
  if (Math.abs(displayAR - tierAR) < 1e-6) return 10000
  return Math.round((displayAR / tierAR) * 10000)
}

/**
 * DPI scaling for Android Auto.
 *
 * Calibrated so each canonical AA tier resolution produces the exact target
 * dpi below (= AA's recommended density per tier on typical car displays):
 *
 *   - 800×480    -> 140 dpi
 *   - 1280×720   -> 180 dpi
 *   - 1920×1080  -> 200 dpi
 *   - 2560×1440  -> 250 dpi
 *   - 3840×2160  -> 420 dpi
 *
 */

const AA_DPI_TIERS: ReadonlyArray<{ pixels: number; dpi: number }> = [
  { pixels: 800 * 480, dpi: 140 },
  { pixels: 1280 * 720, dpi: 180 },
  { pixels: 1920 * 1080, dpi: 200 },
  { pixels: 2560 * 1440, dpi: 250 },
  { pixels: 3840 * 2160, dpi: 420 }
]

export function computeAndroidAutoDpi(width: number, height: number): number {
  const pixels = width * height

  if (pixels <= AA_DPI_TIERS[0].pixels) return AA_DPI_TIERS[0].dpi
  const top = AA_DPI_TIERS[AA_DPI_TIERS.length - 1]
  if (pixels >= top.pixels) return top.dpi

  for (let i = 0; i < AA_DPI_TIERS.length - 1; i++) {
    const lo = AA_DPI_TIERS[i]
    const hi = AA_DPI_TIERS[i + 1]
    if (pixels >= lo.pixels && pixels <= hi.pixels) {
      const t = (pixels - lo.pixels) / (hi.pixels - lo.pixels)
      const dpi = lo.dpi + t * (hi.dpi - lo.dpi)
      return Math.round(dpi / 10) * 10
    }
  }

  return top.dpi
}
