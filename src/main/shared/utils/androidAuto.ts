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
 * DPI scaling for Android Auto
 * - 800×480   -> 140 dpi
 * - 1280×720  -> 160 dpi
 * - 1920×1080 -> 200 dpi
 * - 2560×1440 -> 250 dpi
 * - 3840×2160 -> 420 dpi
 */

export function computeAndroidAutoDpi(width: number, height: number): number {
  const pixels = width * height

  const minPixels = 800 * 480
  const maxPixels = 3840 * 2160

  if (pixels <= minPixels) {
    return 140
  }

  const t = Math.min((pixels - minPixels) / (maxPixels - minPixels), 1)

  let dpi = Math.round(140 + t * (420 - 140))
  dpi = Math.round(dpi / 10) * 10

  return dpi
}
