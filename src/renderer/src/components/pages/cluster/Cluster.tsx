import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import { Box, Typography, useTheme } from '@mui/material'
import { createRenderWorker } from '@worker/createRenderWorker'
import {
  InitEvent,
  SetCodecEvent,
  UpdateHwAccelEvent,
  type VideoCodec
} from '@worker/render/RenderEvents'
import React, { useEffect, useMemo, useRef, useState } from 'react'
import { useLiviStore, useStatusStore } from '../../../store/store'

type ClusterProps = { visible?: boolean }

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

export const Cluster: React.FC<ClusterProps> = ({ visible }) => {
  const theme = useTheme()
  const showCluster = visible === true

  const settings = useLiviStore((s) => s.settings)
  const boxInfoRaw = useLiviStore((s) => s.boxInfo)
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const isAaActive = useStatusStore((s) => s.isAaActive)

  const initialFpsRef = useRef<number | undefined>(settings?.clusterFps)
  if (initialFpsRef.current === undefined) {
    const f = settings?.clusterFps
    if (typeof f === 'number' && f > 0) initialFpsRef.current = f
  }

  const [renderReady, setRenderReady] = useState(false)
  const [rendererError, setRendererError] = useState<string | null>(null)
  const [navHidden, setNavHidden] = useState(false)
  const [clusterStreamActive, setClusterStreamActive] = useState(false)

  const rootRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const renderWorkerRef = useRef<Worker | null>(null)
  const offscreenCanvasRef = useRef<OffscreenCanvas | null>(null)
  const clusterCodecRef = useRef<VideoCodec>('h264')

  const clusterVideoChannel = useMemo(() => new MessageChannel(), [])

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
    // AA-native always exposes a cluster sink (ch=19, display_type=CLUSTER)
    // when clusterEnabled is on — phone streams H.264 cluster frames there.
    if (isAaActive) return true

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
  }, [boxInfoRaw, isAaActive])

  // Request/Release cluster stream + reset worker on disconnect
  const wasStreamingRef = useRef(isStreaming)
  useEffect(() => {
    if (!isStreaming) {
      void window.projection.ipc.requestCluster(false).catch(() => {})
      if (wasStreamingRef.current) {
        clusterCodecRef.current = 'h264'
        try {
          renderWorkerRef.current?.postMessage(new SetCodecEvent('h264'))
          renderWorkerRef.current?.postMessage({ type: 'reset' })
        } catch {
          /* worker not yet alive */
        }
      }
      wasStreamingRef.current = false
      return
    }
    wasStreamingRef.current = true
    if (!renderReady) {
      void window.projection.ipc.requestCluster(false).catch(() => {})
      return
    }
    void window.projection.ipc.requestCluster(true).catch(() => {})
    return () => {
      void window.projection.ipc.requestCluster(false).catch(() => {})
    }
  }, [isStreaming, renderReady])

  // Init Render.worker
  useEffect(() => {
    const targetFps = initialFpsRef.current
    if (typeof targetFps !== 'number' || targetFps <= 0) return

    if (!canvasRef.current) return
    if (offscreenCanvasRef.current || renderWorkerRef.current) return

    offscreenCanvasRef.current = canvasRef.current.transferControlToOffscreen()

    const w = createRenderWorker()
    renderWorkerRef.current = w

    w.postMessage(
      new InitEvent(
        offscreenCanvasRef.current,
        clusterVideoChannel.port2,
        targetFps,
        clusterCodecRef.current,
        Boolean(settings?.hwAcceleration)
      ),
      [offscreenCanvasRef.current, clusterVideoChannel.port2]
    )

    return () => {
      renderWorkerRef.current?.terminate()
      renderWorkerRef.current = null
      offscreenCanvasRef.current = null
    }
  }, [clusterVideoChannel])

  useEffect(() => {
    if (!renderWorkerRef.current) return
    renderWorkerRef.current.postMessage(new UpdateHwAccelEvent(Boolean(settings?.hwAcceleration)))
  }, [settings?.hwAcceleration])

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

      if (t === 'awaiting-keyframe' || t === 'request-keyframe') {
        void window.projection.ipc.requestCluster(true).catch(() => {})
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

  // Listen for cluster-video-codec events to switch the worker's parser
  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const d = args[0] as { type?: string; payload?: { codec?: unknown } } | undefined
      if (d?.type !== 'cluster-video-codec') return
      const codec = d.payload?.codec
      console.log(`[MAPS] cluster-video-codec event received: codec=${codec}`)
      if (codec === 'h264' || codec === 'h265' || codec === 'vp9' || codec === 'av1') {
        if (codec !== clusterCodecRef.current) {
          console.log(`[MAPS] switching worker codec ${clusterCodecRef.current} → ${codec}`)
          clusterCodecRef.current = codec
          renderWorkerRef.current?.postMessage(new SetCodecEvent(codec))
        }
      }
    }
    window.projection.ipc.onEvent(handler)
    return () => window.projection.ipc.offEvent(handler)
  }, [])

  // Forward maps video chunks to Render.worker port
  useEffect(() => {
    const handleVideo = (payload: unknown) => {
      if (rendererError) return
      if (!renderReady || !payload || typeof payload !== 'object') return

      const m = payload as { chunk?: { buffer?: ArrayBuffer } }
      const buf = m.chunk?.buffer
      if (!buf) return

      if (!clusterStreamActive) setClusterStreamActive(true)
      clusterVideoChannel.port1.postMessage(buf, [buf])
    }

    window.projection.ipc.onClusterVideoChunk(handleVideo)
    return () => {}
  }, [clusterVideoChannel, renderReady, rendererError, clusterStreamActive])

  // Reset stream-active flag on disconnect so the placeholder reappears.
  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as { type?: string }
      if (msg.type === 'unplugged' || msg.type === 'failure') {
        setClusterStreamActive(false)
      }
    }
    window.projection.ipc.onEvent(handler)
    return () => window.projection.ipc.offEvent(handler)
  }, [])

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
        backgroundColor: theme.palette.background.default,
        visibility: showCluster ? 'visible' : 'hidden',
        opacity: showCluster ? 1 : 0,
        pointerEvents: showCluster ? 'auto' : 'none',
        transition: 'opacity 220ms ease',
        zIndex: showCluster ? 5 : -1
      }}
    >
      {!clusterStreamActive && showCluster && (
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            display: 'grid',
            placeItems: 'center',
            textAlign: 'center',
            pointerEvents: 'none',
            zIndex: 6,
            backgroundColor: theme.palette.background.default
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
              pointerEvents: 'none',
              background: '#000'
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
