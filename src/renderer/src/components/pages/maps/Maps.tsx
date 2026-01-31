import React, { useEffect, useMemo, useRef, useState } from 'react'
import { Box, Typography, useTheme } from '@mui/material'
import { InitEvent } from '@worker/render/RenderEvents'
import { useStatusStore, useCarplayStore } from '../../../store/store'

export const Maps: React.FC = () => {
  const theme = useTheme()

  const settings = useCarplayStore((s) => s.settings)
  const boxInfoRaw = useCarplayStore((s) => s.boxInfo)
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const fps = settings?.fps

  const [renderReady, setRenderReady] = useState(false)
  const [rendererError, setRendererError] = useState<string | null>(null)

  const rootRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const renderWorkerRef = useRef<Worker | null>(null)
  const offscreenCanvasRef = useRef<OffscreenCanvas | null>(null)

  const mapsVideoChannel = useMemo(() => new MessageChannel(), [])

  // Check for feature flag
  const supportsNaviScreen = useMemo(() => {
    let box: any = null
    if (boxInfoRaw && typeof boxInfoRaw === 'object') {
      box = boxInfoRaw as any
    } else if (typeof boxInfoRaw === 'string') {
      const s = boxInfoRaw.trim()
      if (!s) return false
      try {
        const parsed = JSON.parse(s)
        if (parsed && typeof parsed === 'object') box = parsed
      } catch {
        return false
      }
    }

    if (!box) return false
    const features = box.supportFeatures

    if (Array.isArray(features)) {
      return features.some((f) => String(f).trim().toLowerCase() === 'naviscreen')
    }
    if (typeof features === 'string') {
      return features
        .split(/[,\s]+/g)
        .map((x) => x.trim().toLowerCase())
        .filter(Boolean)
        .includes('naviscreen')
    }
    return false
  }, [boxInfoRaw])

  // Request/Release maps stream
  useEffect(() => {
    let cancelled = false

    const apply = async () => {
      try {
        const enabled = Boolean(isStreaming && supportsNaviScreen)
        await window.carplay.ipc.requestMaps(enabled)
        if (cancelled) return
      } catch {
        // ignore
      }
    }

    void apply()

    return () => {
      cancelled = true
      void window.carplay.ipc.requestMaps(false).catch(() => {})
    }
  }, [isStreaming, supportsNaviScreen])

  // Init Render.worker
  useEffect(() => {
    if (typeof fps !== 'number' || fps <= 0) return

    if (!canvasRef.current) return
    if (offscreenCanvasRef.current || renderWorkerRef.current) return

    offscreenCanvasRef.current = canvasRef.current.transferControlToOffscreen()

    const w = new Worker(new URL('../../worker/render/Render.worker.ts', import.meta.url), {
      type: 'module'
    })
    renderWorkerRef.current = w

    w.postMessage(new InitEvent(offscreenCanvasRef.current, mapsVideoChannel.port2, fps), [
      offscreenCanvasRef.current,
      mapsVideoChannel.port2
    ])

    return () => {
      renderWorkerRef.current?.terminate()
      renderWorkerRef.current = null
      offscreenCanvasRef.current = null
    }
  }, [mapsVideoChannel, fps])

  // Render.worker ready/error messages
  useEffect(() => {
    const w = renderWorkerRef.current
    if (!w) return

    const handler = (ev: MessageEvent<any>) => {
      const t = ev.data?.type
      if (t === 'render-ready') {
        setRenderReady(true)
        setRendererError(null)
        return
      }
      if (t === 'render-error') {
        const msg =
          typeof ev.data?.message === 'string' && ev.data.message.trim()
            ? ev.data.message.trim()
            : 'No renderer available'
        setRendererError(msg)
        setRenderReady(false)
        w.postMessage({ type: 'clear' })
      }
    }

    w.addEventListener('message', handler)
    return () => w.removeEventListener('message', handler)
  }, [])

  // resize
  useEffect(() => {
    const w = renderWorkerRef.current
    const el = rootRef.current
    if (!w || !el) return

    const poke = () => {
      w.postMessage({ type: 'frame' })
    }

    // do one immediately
    poke()

    const ro = new ResizeObserver(poke)
    ro.observe(el)

    document.addEventListener('fullscreenchange', poke)
    window.addEventListener('resize', poke)

    return () => {
      ro.disconnect()
      document.removeEventListener('fullscreenchange', poke)
      window.removeEventListener('resize', poke)
    }
  }, [renderReady])

  // Forward maps video chunks to Render.worker port
  useEffect(() => {
    const handleVideo = (payload: unknown) => {
      if (rendererError) return
      if (!renderReady || !payload || typeof payload !== 'object') return

      const m = payload as { chunk?: { buffer?: ArrayBuffer } }
      const buf = m.chunk?.buffer
      if (!buf) return

      mapsVideoChannel.port1.postMessage(buf, [buf])
    }

    window.carplay.ipc.onMapsVideoChunk(handleVideo)
    return () => {}
  }, [mapsVideoChannel, renderReady, rendererError])

  const canShowVideo = !rendererError

  return (
    <Box
      ref={rootRef}
      sx={{
        position: 'fixed',
        inset: 0,
        width: '100vw',
        height: '100vh',
        overflow: 'hidden',
        display: 'flex',
        justifyContent: 'stretch',
        alignItems: 'stretch',
        backgroundColor: theme.palette.background.default
      }}
    >
      {!isStreaming && (
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            display: 'grid',
            placeItems: 'start center',
            pt: 6,
            pl: { xs: '84px', sm: '92px' },
            textAlign: 'center',
            pointerEvents: 'none'
          }}
        >
          <Box sx={{ px: 2 }}>
            <Typography variant="h6">Maps</Typography>
            <Typography variant="body2" sx={{ opacity: 0.8, mt: 1 }}>
              Start CarPlay to enable the navigation screen.
            </Typography>
          </Box>
        </Box>
      )}

      {isStreaming && supportsNaviScreen && (
        <Box
          sx={{
            width: '100%',
            height: '100%',
            display: canShowVideo ? 'flex' : 'none',
            justifyContent: 'center',
            alignItems: 'flex-start'
          }}
        >
          <Box
            sx={{
              width: '100%',
              height: '100%',
              maxWidth: '100%'
            }}
          >
            <canvas
              ref={canvasRef}
              style={{
                width: '100%',
                height: '100%',
                display: 'block',
                userSelect: 'none',
                pointerEvents: 'none'
              }}
            />
          </Box>
        </Box>
      )}

      {isStreaming && !supportsNaviScreen && (
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            display: 'grid',
            placeItems: 'start center',
            pt: 6,
            pl: { xs: '84px', sm: '92px' },
            textAlign: 'center',
            pointerEvents: 'none'
          }}
        >
          <Box sx={{ px: 2 }}>
            <Typography variant="h6">Maps</Typography>
            <Typography variant="body2" sx={{ opacity: 0.8, mt: 1 }}>
              This dongle firmware does not support the navigation screen (NaviScreen).
            </Typography>
          </Box>
        </Box>
      )}

      {rendererError && (
        <Box sx={{ position: 'absolute', top: 16, left: 16, right: 16 }}>
          <Typography variant="body2" color="error">
            {rendererError}
          </Typography>
        </Box>
      )}
    </Box>
  )
}
