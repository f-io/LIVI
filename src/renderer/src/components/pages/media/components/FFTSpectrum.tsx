import { Box } from '@mui/material'
import { alpha, useTheme } from '@mui/material/styles'
import { useLiviStore } from '@store/store'
import { useEffect, useRef, useState } from 'react'
import { createFftWorker } from './createFftWorker'

// Configuration
const POINTS = 24
const FFT_SIZE = 4096
const LABEL_FONT_MAX = 16
const LABEL_FONT_MIN = 9
const MIN_FREQ = 20
const MAX_FREQ = 20000
const SPECTRUM_WIDTH_RATIO = 1.0
const TARGET_FPS = 60

// Label font scales with spectrum width; below LABEL_FONT_MIN labels are dropped and the margin freed.
const labelMetrics = (specW: number) => {
  const font = Math.min(LABEL_FONT_MAX, Math.floor(specW / 12))
  const show = font >= LABEL_FONT_MIN
  return { font, show, marginBottom: show ? font + 4 : 0 }
}

export const normalizePcmBuffer = (pcm: Float32Array | ArrayLike<number>) => {
  return pcm instanceof Float32Array ? pcm.slice() : new Float32Array(pcm)
}

export const FFTSpectrum = () => {
  const theme = useTheme()
  const barColor =
    getComputedStyle(document.body).getPropertyValue('--ui-highlight').trim() ||
    theme.palette.primary.main

  const labelColor = alpha(theme.palette.text.secondary, 0.9)

  const canvasRef = useRef<HTMLCanvasElement>(null)
  const bgCanvasRef = useRef<HTMLCanvasElement>(null)

  const sampleRate = useLiviStore((s) => s.audioSampleRate) ?? 48000
  const visualAudioDelayMs = useLiviStore((s) => s.visualAudioDelayMs) ?? 120
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 })

  const workerRef = useRef<Worker | null>(null)
  const binsRef = useRef<Float32Array>(new Float32Array(POINTS))
  const timeoutsRef = useRef<number[]>([])

  useEffect(() => {
    const worker = createFftWorker()
    workerRef.current = worker
    worker.postMessage({
      type: 'init',
      fftSize: FFT_SIZE,
      points: POINTS,
      sampleRate,
      minFreq: MIN_FREQ,
      maxFreq: MAX_FREQ
    })
    worker.onmessage = (e: MessageEvent) => {
      if (e.data.type === 'bins') {
        binsRef.current = new Float32Array(e.data.bins)
      }
    }
    return () => worker.terminate()
  }, [sampleRate])

  useEffect(() => {
    const unsubscribe = useLiviStore.subscribe((state) => {
      const pcm = state.audioPcmData
      const worker = workerRef.current
      if (!worker || !pcm || pcm.length === 0) return

      const buf = normalizePcmBuffer(pcm)

      if (visualAudioDelayMs > 0) {
        const id = window.setTimeout(() => {
          worker.postMessage({ type: 'pcm', buffer: buf.buffer }, [buf.buffer])
        }, visualAudioDelayMs)
        timeoutsRef.current.push(id)
      } else {
        worker.postMessage({ type: 'pcm', buffer: buf.buffer }, [buf.buffer])
      }
    })

    return () => {
      unsubscribe()
      timeoutsRef.current.forEach((id) => clearTimeout(id))
      timeoutsRef.current = []
    }
  }, [visualAudioDelayMs])

  useEffect(() => {
    const canvas = canvasRef.current as HTMLCanvasElement

    const update = () => {
      const { width, height } = canvas.getBoundingClientRect()
      setDimensions({ width, height })
    }

    const obs = new ResizeObserver(update)
    obs.observe(canvas)
    return () => obs.disconnect()
  }, [])

  useEffect(() => {
    const bg = bgCanvasRef.current
    if (!bg || dimensions.width === 0) return
    const ctx = bg.getContext('2d')!
    const { width: cw, height: ch } = dimensions
    const specW = cw * SPECTRUM_WIDTH_RATIO
    const { font: labelFont, show: showLabels, marginBottom } = labelMetrics(specW)
    const usableH = ch - marginBottom
    const xOff = (cw - specW) / 2
    bg.width = cw
    bg.height = ch

    ctx.clearRect(0, 0, cw, ch)

    const freqs = [MIN_FREQ, 100, 1000, 10000, MAX_FREQ]
    const logMin = Math.log10(MIN_FREQ)
    const logMax = Math.log10(MAX_FREQ)
    const logDen = logMax - logMin

    const positions = freqs.map((freq) => ({
      freq,
      x: xOff + ((Math.log10(freq) - logMin) / logDen) * specW
    }))

    if (showLabels) {
      ctx.font = `${labelFont}px sans-serif`
      ctx.textBaseline = 'top'
      ctx.fillStyle = labelColor
      const gap = labelFont * 0.5
      const items = positions.map(({ freq, x }, i) => {
        const label = freq >= 1000 ? `${freq / 1000}k` : `${freq}`
        const w = ctx.measureText(label).width
        const align: CanvasTextAlign =
          i === 0 ? 'left' : i === positions.length - 1 ? 'right' : 'center'
        const left = align === 'left' ? x : align === 'right' ? x - w : x - w / 2
        return { label, x, align, left, right: left + w }
      })
      // Right-to-left pass: draw a label only if it clears the last drawn one.
      let lastLeft = Infinity
      for (let i = items.length - 1; i >= 0; i--) {
        const it = items[i]
        if (it.right <= lastLeft - gap) {
          ctx.textAlign = it.align
          ctx.fillText(it.label, it.x, usableH + 2)
          lastLeft = it.left
        }
      }
    }
  }, [dimensions, sampleRate, labelColor])

  useEffect(() => {
    let rafId = 0
    let last = 0

    const draw = () => {
      rafId = requestAnimationFrame(draw)
      const now = performance.now()
      if (now - last < 1000 / TARGET_FPS) return
      last = now

      const canvas = canvasRef.current
      if (!canvas) return
      const ctx = canvas.getContext('2d')!
      const { width: cw, height: ch } = dimensions
      if (cw === 0 || ch === 0) return

      if (canvas.width !== cw || canvas.height !== ch) {
        canvas.width = cw
        canvas.height = ch
      }

      ctx.clearRect(0, 0, cw, ch)
      const specW = cw * SPECTRUM_WIDTH_RATIO
      const usableH = ch - labelMetrics(specW).marginBottom
      const xOff = (cw - specW) / 2
      const barW = specW / POINTS
      const bins = binsRef.current

      for (let i = 0; i < POINTS; i++) {
        const h = bins[i] * usableH
        const x = xOff + i * barW
        ctx.fillStyle = barColor
        ctx.fillRect(x, usableH - h, barW * 0.8, h)
      }
    }

    draw()

    return () => {
      globalThis.cancelAnimationFrame?.(rafId)
    }
  }, [dimensions, barColor])

  return (
    <Box sx={{ position: 'relative', width: '100%', height: '100%' }}>
      <Box
        ref={bgCanvasRef}
        component="canvas"
        sx={{
          position: 'absolute',
          width: '100%',
          height: '100%',
          pointerEvents: 'none',
          zIndex: 1
        }}
      />
      <Box
        ref={canvasRef}
        component="canvas"
        sx={{
          position: 'absolute',
          width: '100%',
          height: '100%',
          background: 'transparent',
          zIndex: 2
        }}
      />
    </Box>
  )
}
