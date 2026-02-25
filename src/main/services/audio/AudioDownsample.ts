export interface DownsampleOptions {
  inSampleRate: number
  inChannels: number
  outSampleRate?: number
}

export function downsampleToMono(pcm: Int16Array, opts: DownsampleOptions): Int16Array {
  const { inSampleRate, inChannels, outSampleRate = inSampleRate } = opts

  if (!pcm || pcm.length === 0) return new Int16Array(0)
  if (inChannels <= 0) return new Int16Array(0)

  if (inChannels === 1 && inSampleRate === outSampleRate) {
    return pcm
  }

  const framesIn = Math.floor(pcm.length / inChannels)
  if (framesIn <= 0) return new Int16Array(0)

  const ratio = inSampleRate / outSampleRate
  if (!Number.isFinite(ratio) || ratio <= 0) {
    return new Int16Array(0)
  }

  const framesOut = Math.floor(framesIn / ratio)
  if (framesOut <= 0) return new Int16Array(0)

  const out = new Int16Array(framesOut)

  let pos = 0
  for (let i = 0; i < framesOut; i++) {
    const srcFrame = Math.floor(pos)
    const baseIndex = srcFrame * inChannels
    if (baseIndex >= pcm.length) break

    let sum = 0
    for (let c = 0; c < inChannels; c++) {
      sum += pcm[baseIndex + c] || 0
    }

    out[i] = (sum / inChannels) | 0
    pos += ratio
  }

  return out
}
