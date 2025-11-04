import { useEffect, useRef, useState } from 'react'
import { Box } from '@mui/material'
import { useCarplayStore } from '@store/store'
import { useTheme, alpha } from '@mui/material/styles'

export interface FFTSpectrumProps {
  data: number[] | Float32Array
}

// Configuration
const POINTS = 24
const FFT_SIZE = 4096
const LABEL_FONT_SIZE = 16
const MARGIN_BOTTOM = 16
const MIN_FREQ = 20
const MAX_FREQ = 20000
const SPECTRUM_WIDTH_RATIO = 0.92
const TARGET_FPS = 30

export const FFTSpectrum = ({ data }: FFTSpectrumProps) => {
  const theme = useTheme()
  const isDark = theme.palette.mode === 'dark'

  const barColor = theme.palette.primary.main

  // Grid/labels derived from theme text colors
  const gridFill = alpha(theme.palette.text.primary, isDark ? 0.12 : 0.06)
  const gridLine = alpha(theme.palette.text.primary, 0.35)
  const majorLine = alpha(theme.palette.text.primary, 0.45)
  const labelColor = alpha(theme.palette.text.secondary, 0.9)

  const canvasRef = useRef<HTMLCanvasElement>(null)
  const bgCanvasRef = useRef<HTMLCanvasElement>(null)
  const dataRef = useRef<number[] | Float32Array>(data)
  dataRef.current = data

  const sampleRate = useCarplayStore((s) => s.audioSampleRate) ?? 44100
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 })

  // Worker and buffers
  const workerRef = useRef<Worker | null>(null)
  const binsRef = useRef<Float32Array>(new Float32Array(POINTS))

  useEffect(() => {
    const worker = new Worker(new URL('../../worker/fft.worker.ts', import.meta.url), {
      type: 'module'
    })
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
    const worker = workerRef.current
    if (!worker || dataRef.current.length === 0) return
    const buf =
      dataRef.current instanceof Float32Array ? dataRef.current : new Float32Array(dataRef.current)
    worker.postMessage({ type: 'pcm', buffer: buf.buffer }, [buf.buffer])
  }, [data])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const update = () => {
      const { width, height } = canvas.getBoundingClientRect()
      setDimensions({ width, height })
    }
    const obs = new ResizeObserver(update)
    obs.observe(canvas)
    return () => obs.disconnect()
  }, [])

  // Static grid and labels (redrawn on theme, size, or sample rate changes)
  useEffect(() => {
    const bg = bgCanvasRef.current
    if (!bg || dimensions.width === 0) return
    const ctx = bg.getContext('2d')!
    const { width: cw, height: ch } = dimensions
    const usableH = ch - MARGIN_BOTTOM
    const specW = cw * SPECTRUM_WIDTH_RATIO
    const xOff = (cw - specW) / 2
    bg.width = cw
    bg.height = ch

    ctx.clearRect(0, 0, cw, ch)

    // Background band
    ctx.fillStyle = gridFill
    ctx.fillRect(xOff, 0, specW, usableH)

    // Horizontal guide lines
    ctx.lineWidth = 0.5
    ;[0.25, 0.5, 0.75].forEach((f) => {
      const y = usableH * f
      ctx.strokeStyle = gridLine
      ctx.beginPath()
      ctx.moveTo(xOff, y)
      ctx.lineTo(xOff + specW, y)
      ctx.stroke()
    })

    // Vertical lines + labels
    const freqs = [MIN_FREQ, 100, 500, 1000, 5000, 10000, MAX_FREQ]
    ctx.font = `${LABEL_FONT_SIZE}px sans-serif`
    ctx.textAlign = 'center'
    ctx.textBaseline = 'top'
    ctx.fillStyle = labelColor

    const logMin = Math.log10(MIN_FREQ)
    const logMax = Math.log10(MAX_FREQ)
    const logDen = logMax - logMin

    freqs.forEach((freq) => {
      const pos = (Math.log10(freq) - logMin) / logDen
      const x = xOff + pos * specW
      ctx.strokeStyle = majorLine
      ctx.beginPath()
      ctx.moveTo(x, 0)
      ctx.lineTo(x, usableH)
      ctx.stroke()
      const label = freq >= 1000 ? `${freq / 1000}k` : `${freq}`
      ctx.fillText(label, x, usableH + 2)
    })
  }, [dimensions, sampleRate, gridFill, gridLine, majorLine, labelColor])

  // Dynamic bars only
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
      if (canvas.width !== cw || canvas.height !== ch) {
        canvas.width = cw
        canvas.height = ch
      }

      ctx.clearRect(0, 0, cw, ch)
      const usableH = ch - MARGIN_BOTTOM
      const specW = cw * SPECTRUM_WIDTH_RATIO
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
    return () => cancelAnimationFrame(rafId)
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
