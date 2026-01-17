import { useEffect, useRef, useState } from 'react'
import { Box } from '@mui/material'
import { useCarplayStore } from '@store/store'
import { useTheme, alpha } from '@mui/material/styles'

// Configuration
const POINTS = 24
const FFT_SIZE = 4096
const LABEL_FONT_SIZE = 16
const MARGIN_BOTTOM = 16
const MIN_FREQ = 20
const MAX_FREQ = 20000
const SPECTRUM_WIDTH_RATIO = 0.92
const TARGET_FPS = 30

export const FFTSpectrum = () => {
  const theme = useTheme()
  const isDark = theme.palette.mode === 'dark'

  const barColor =
    getComputedStyle(document.body).getPropertyValue('--ui-highlight').trim() ||
    theme.palette.primary.main

  const gridFill = alpha(theme.palette.text.primary, isDark ? 0.12 : 0.06)
  const gridLine = alpha(theme.palette.text.primary, 0.35)
  const majorLine = alpha(theme.palette.text.primary, 0.45)
  const labelColor = alpha(theme.palette.text.secondary, 0.9)

  const canvasRef = useRef<HTMLCanvasElement>(null)
  const bgCanvasRef = useRef<HTMLCanvasElement>(null)

  const sampleRate = useCarplayStore((s) => s.audioSampleRate) ?? 48000
  const visualAudioDelayMs = useCarplayStore((s) => s.visualAudioDelayMs) ?? 120
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 })

  const workerRef = useRef<Worker | null>(null)
  const binsRef = useRef<Float32Array>(new Float32Array(POINTS))
  const timeoutsRef = useRef<number[]>([])

  useEffect(() => {
    const worker = new Worker(new URL('@worker/fft.worker.ts', import.meta.url), {
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
    const unsubscribe = useCarplayStore.subscribe((state) => {
      const pcm = state.audioPcmData
      const worker = workerRef.current
      if (!worker || !pcm || pcm.length === 0) return

      // Copy to avoid transferring the buffer from the store directly
      const buf = pcm instanceof Float32Array ? pcm.slice() : new Float32Array(pcm)

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

    ctx.fillStyle = gridFill
    ctx.fillRect(xOff, 0, specW, usableH)

    ctx.lineWidth = 0.5
    ;[0.25, 0.5, 0.75].forEach((f) => {
      const y = usableH * f
      ctx.strokeStyle = gridLine
      ctx.beginPath()
      ctx.moveTo(xOff, y)
      ctx.lineTo(xOff + specW, y)
      ctx.stroke()
    })

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
