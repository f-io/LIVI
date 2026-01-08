import { MIN_ART_COL, MIN_TEXT_COL } from '../constants'
import { clamp } from './clamp'

export const mediaLayoutArtworksOps = ({
  ctrlSize,
  progressH,
  w,
  h,
  pagePad,
  colGap,
  titlePx,
  artistPx,
  albumPx
}: {
  ctrlSize: number
  progressH: number
  w: number
  h: number
  pagePad: number
  colGap: number
  titlePx: number
  artistPx: number
  albumPx: number
}) => {
  const bottomDockH = ctrlSize + 16 + (progressH + 20)
  const contentH = Math.max(0, h - pagePad * 2 - bottomDockH)
  const innerW = Math.max(0, w - pagePad * 2)
  const canTwoCol = innerW >= MIN_TEXT_COL + MIN_ART_COL + colGap
  const textEst = titlePx * 1.25 + artistPx * 1.25 + albumPx * 1.1 + 40
  const artFromH = Math.max(130, contentH - Math.max(60, Math.min(textEst, contentH * 0.6)))
  const artWidthAllowance = Math.max(MIN_ART_COL, Math.floor(innerW - MIN_TEXT_COL - colGap))
  const artPx = canTwoCol
    ? Math.round(clamp(Math.min(contentH, artWidthAllowance), 140, 340))
    : Math.round(clamp(Math.min(h * 0.52, artFromH), 130, 320))

  return {
    bottomDockH,
    contentH,
    innerW,
    canTwoCol,
    textEst,
    artFromH,
    artWidthAllowance,
    artPx
  }
}
