import React, { useEffect, useMemo, useRef, useState } from 'react'
import { Box, Typography, useTheme } from '@mui/material'
import { InitEvent } from '@worker/render/RenderEvents'
import { useStatusStore, useCarplayStore } from '../../../store/store'
import MapOutlinedIcon from '@mui/icons-material/MapOutlined'

type BoxInfo = { supportFeatures?: unknown }

function isBoxInfo(v: unknown): v is BoxInfo {
  return typeof v === 'object' && v !== null
}

function parseBoxInfo(raw: unknown): BoxInfo | null {
  if (isBoxInfo(raw)) return raw

  if (typeof raw === 'string') {
    const s = raw.trim()
    if (!s) return null
    try {
      const parsed: unknown = JSON.parse(s)
      return isBoxInfo(parsed) ? parsed : null
    } catch {
      return null
    }
  }

  return null
}

export const Maps: React.FC = () => {
  const theme = useTheme()

  const settings = useCarplayStore((s) => s.settings)
  const boxInfoRaw = useCarplayStore((s) => s.boxInfo)
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const fps = settings?.fps

  const [renderReady, setRenderReady] = useState(false)
  const [rendererError, setRendererError] = useState<string | null>(null)
  const [navHidden, setNavHidden] = useState(false)

  const rootRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const renderWorkerRef = useRef<Worker | null>(null)
  const offscreenCanvasRef = useRef<OffscreenCanvas | null>(null)

  const mapsVideoChannel = useMemo(() => new MessageChannel(), [])

  useEffect(() => {
    const el = document.getElementById('content-root')
    if (!el) return

    const read = () => setNavHidden(el.getAttribute('data-nav-hidden') === '1')
    read()

    const mo = new MutationObserver(read)
    mo.observe(el, { attributes: true, attributeFilter: ['data-nav-hidden'] })

    return () => mo.disconnect()
  }, [])

  // Render.worker message typing
  type RenderWorkerMsg =
    | { type: 'render-ready' }
    | { type: 'render-error'; message?: string }
    | { type: string; [key: string]: unknown }

  function isRecord(v: unknown): v is Record<string, unknown> {
    return typeof v === 'object' && v !== null
  }

  const readWorkerMsg = React.useCallback((data: unknown): RenderWorkerMsg | null => {
    if (!isRecord(data)) return null
    const t = data.type
    if (typeof t !== 'string') return null
    return data as RenderWorkerMsg
  }, [])

  const supportsNaviScreen = useMemo(() => {
    const box = parseBoxInfo(boxInfoRaw)
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
        const enabled = Boolean(isStreaming)
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
  }, [isStreaming])

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

    const handler = (ev: MessageEvent<unknown>) => {
      const msg = readWorkerMsg(ev.data)
      const t = msg?.type

      if (t === 'render-ready') {
        setRenderReady(true)
        setRendererError(null)
        console.log('[MAPS] Render worker ready message received')
        return
      }

      if (t === 'render-error') {
        const message = msg && typeof msg.message === 'string' ? msg.message.trim() : ''
        const text = message ? message : 'No renderer available'
        setRendererError(text)
        setRenderReady(false)
        w.postMessage({ type: 'clear' })
      }
    }

    w.addEventListener('message', handler)
    return () => w.removeEventListener('message', handler)
  }, [readWorkerMsg])

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
        position: navHidden ? 'fixed' : 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
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
            placeItems: 'center',
            textAlign: 'center',
            pointerEvents: 'none'
          }}
        >
          <MapOutlinedIcon sx={{ fontSize: 84, opacity: 0.55 }} />
        </Box>
      )}

      {/* Canvas is ALWAYS mounted so the renderer can init immediately*/}
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

      {isStreaming && !supportsNaviScreen && (
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            display: 'grid',
            placeItems: 'center',
            textAlign: 'center',
            pointerEvents: 'none'
          }}
        >
          <Box sx={{ display: 'grid', placeItems: 'center', gap: 1 }}>
            <MapOutlinedIcon sx={{ fontSize: 84, opacity: 0.55 }} />
            <Typography variant="body2" sx={{ opacity: 0.75 }}>
              Not supported by firmware
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
