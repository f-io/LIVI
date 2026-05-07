export const clamp = (number: number, min: number, max: number) => {
  return Math.max(min, Math.min(number, max))
}

export function getCurrentTimeInMs() {
  return Math.round(Date.now() / 1000)
}

export type AndroidAutoResolution = {
  width: number
  height: number
}

export function dongleDisplayName(name: string): string {
  return `${name} (D)`
}

/**
 * Android Auto resolution selection
 * - tier chosen by width
 * - height derived from display aspect ratio
 * - height forced even
 * - clamped to tier height
 */
export function matchFittingAAResolution(userRes: {
  width: number
  height: number
}): AndroidAutoResolution {
  const w = userRes.width
  const h = userRes.height
  const displayAR = w / h

  let tierWidth = 800
  let tierHeight = 480

  if (w >= 3840) {
    tierWidth = 3840
    tierHeight = 2160
  } else if (w >= 2560) {
    tierWidth = 2560
    tierHeight = 1440
  } else if (w >= 1920) {
    tierWidth = 1920
    tierHeight = 1080
  } else if (w >= 1280) {
    tierWidth = 1280
    tierHeight = 720
  }

  const width = tierWidth
  const height = Math.min(Math.floor(tierWidth / displayAR) & ~1, tierHeight)

  return { width, height }
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

  // Below / at the lowest tier → minimum dpi.
  if (pixels <= AA_DPI_TIERS[0].pixels) return AA_DPI_TIERS[0].dpi
  // At / above the highest tier → maximum dpi.
  const top = AA_DPI_TIERS[AA_DPI_TIERS.length - 1]
  if (pixels >= top.pixels) return top.dpi

  // Walk segments to find the bracketing pair.
  for (let i = 0; i < AA_DPI_TIERS.length - 1; i++) {
    const lo = AA_DPI_TIERS[i]
    const hi = AA_DPI_TIERS[i + 1]
    if (pixels >= lo.pixels && pixels <= hi.pixels) {
      const t = (pixels - lo.pixels) / (hi.pixels - lo.pixels)
      const dpi = lo.dpi + t * (hi.dpi - lo.dpi)
      return Math.round(dpi / 10) * 10
    }
  }

  // Unreachable — guarded by the bounds checks above.
  return top.dpi
}
