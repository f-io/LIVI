import { clamp } from './clamp'

export const mediaScaleOps = ({ w, h }: { w: number; h: number }) => {
  const minSide = Math.min(w, h)
  const titlePx = Math.round(clamp(minSide * 0.07, 22, 48))
  const artistPx = Math.round(clamp(minSide * 0.034, 14, 24))
  const albumPx = Math.round(clamp(minSide * 0.028, 13, 20))
  const pagePad = Math.round(clamp(minSide * 0.02, 12, 22))
  const colGap = Math.round(clamp(w * 0.025, 16, 28))
  const sectionGap = Math.round(clamp(h * 0.03, 10, 24))
  const ctrlSize = Math.round(clamp(h * 0.095, 50, 82))
  const ctrlGap = Math.round(clamp(w * 0.03, 16, 32))
  const progressH = Math.round(clamp(h * 0.012, 8, 12))

  return {
    minSide,
    titlePx,
    artistPx,
    albumPx,
    pagePad,
    colGap,
    sectionGap,
    ctrlSize,
    ctrlGap,
    progressH
  }
}
