import type { CSSProperties } from 'react'

/** Where the video plane draws the stream, so touch reads against the picture, not the bars. */
export function streamBox(width: number, height: number): CSSProperties {
  const centred: CSSProperties = {
    position: 'absolute',
    left: '50%',
    top: '50%',
    transform: 'translate(-50%, -50%)'
  }
  if (!(width > 0 && height > 0)) return { ...centred, width: '100%', height: '100%' }
  return {
    ...centred,
    width: `min(100cqw, calc(100cqh * ${width} / ${height}))`,
    height: `min(100cqh, calc(100cqw * ${height} / ${width}))`
  }
}
