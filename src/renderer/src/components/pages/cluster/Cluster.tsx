import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import { Box, Typography, useTheme } from '@mui/material'
import { aaContentArea } from '@shared/utils'
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
  const [clusterStreamActive, setClusterStreamActive] = useState(false)
  const [clusterFrameSize, setClusterFrameSize] = useState<{ w: number; h: number } | null>(null)

  const rootRef = useRef<HTMLDivElement>(null)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const renderWorkerRef = useRef<Worker | null>(null)
  const offscreenCanvasRef = useRef<OffscreenCanvas | null>(null)
  const clusterCodecRef = useRef<VideoCodec>('h264')

  const clusterVideoChannel = useMemo(() => new MessageChannel(), [])

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
    // AA-native exposes a cluster sink (ch=19, display_type=CLUSTER) when any cluster display is active
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

  const wantCluster =
    settings?.cluster?.main === true ||
    settings?.cluster?.dash === true ||
    settings?.cluster?.aux === true

  useEffect(() => {
    if (!wantCluster) return
    if (!renderReady) return
    void window.projection.ipc.requestCluster(true).catch(() => {})
  }, [renderReady, wantCluster])

  const prevClusterVisibleRef = useRef(false)
  useEffect(() => {
    const wasVisible = prevClusterVisibleRef.current
    prevClusterVisibleRef.current = showCluster
    if (!showCluster || wasVisible) return
    if (!wantCluster || !renderReady) return
    void window.projection.ipc.requestCluster(true).catch(() => {})
  }, [showCluster, wantCluster, renderReady])

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as { type?: string }
      if (msg.type !== 'plugged') return
      if (!wantCluster) return
      if (!renderReady) return
      void window.projection.ipc.requestCluster(true).catch(() => {})
    }
    window.projection.ipc.onEvent(handler)
    return () => window.projection.ipc.offEvent(handler)
  }, [renderReady, wantCluster])

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
      setRenderReady(false)
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
        if (wantCluster) {
          void window.projection.ipc.requestCluster(true).catch(() => {})
        }
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
  }, [readWorkerMsg, wantCluster])

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

  // Forward video chunks to Render.worker port. Register only once the
  // worker is ready so the initial SPS+IDR isn't dropped while it's still
  // spinning up — preload queues chunks until the handler is attached.
  useEffect(() => {
    if (!renderReady || rendererError) return
    const handleVideo = (payload: unknown) => {
      if (!payload || typeof payload !== 'object') return

      const m = payload as { chunk?: { buffer?: ArrayBuffer } }
      const buf = m.chunk?.buffer
      if (!buf) return

      if (!clusterStreamActive) setClusterStreamActive(true)
      clusterVideoChannel.port1.postMessage(buf, [buf])
    }

    window.projection.ipc.onClusterVideoChunk(handleVideo)
    return () => {}
  }, [clusterVideoChannel, renderReady, rendererError, clusterStreamActive])

  // Track the negotiated cluster frame dims so the canvas crop math below
  // matches whatever tier the phone actually picked.
  useEffect(() => {
    const ipc = (window.projection?.ipc ?? {}) as {
      onClusterResolution?: (cb: (payload: unknown) => void) => void
    }
    if (typeof ipc.onClusterResolution !== 'function') return
    ipc.onClusterResolution((payload: unknown) => {
      const d = payload as { width?: number; height?: number } | undefined
      const w = typeof d?.width === 'number' ? d.width : 0
      const h = typeof d?.height === 'number' ? d.height : 0
      if (w > 0 && h > 0) setClusterFrameSize({ w, h })
    })
  }, [])

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as { type?: string }
      if (msg.type !== 'unplugged' && msg.type !== 'failure') return
      setClusterStreamActive(false)
      clusterCodecRef.current = 'h264'
      try {
        renderWorkerRef.current?.postMessage(new SetCodecEvent('h264'))
        renderWorkerRef.current?.postMessage({ type: 'reset' })
      } catch {}
      void window.projection.ipc.requestCluster(false).catch(() => {})
    }
    window.projection.ipc.onEvent(handler)
    return () => window.projection.ipc.offEvent(handler)
  }, [])

  const canShowVideo = !rendererError

  const userClusterW = settings?.clusterWidth ?? 0
  const userClusterH = settings?.clusterHeight ?? 0
  const clusterCrop = (() => {
    if (!clusterFrameSize || userClusterW <= 0 || userClusterH <= 0) return null
    const frameW = clusterFrameSize.w
    const frameH = clusterFrameSize.h
    const content = aaContentArea(
      { width: frameW, height: frameH },
      { width: userClusterW, height: userClusterH }
    )
    const overX = frameW > content.contentWidth ? frameW / content.contentWidth : 1
    const overY = frameH > content.contentHeight ? frameH / content.contentHeight : 1
    const leftPct = ((frameW - content.contentWidth) / 2 / content.contentWidth) * 100
    const topPct = ((frameH - content.contentHeight) / 2 / content.contentHeight) * 100
    return { overX, overY, leftPct, topPct }
  })()

  return (
    <Box
      ref={rootRef}
      sx={{
        position: 'fixed',
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
            maxWidth: '100%',
            position: 'relative',
            overflow: 'hidden'
          }}
        >
          <canvas
            ref={canvasRef}
            style={{
              position: 'absolute',
              width: clusterCrop ? `${clusterCrop.overX * 100}%` : '100%',
              height: clusterCrop ? `${clusterCrop.overY * 100}%` : '100%',
              left: clusterCrop ? `-${clusterCrop.leftPct}%` : '0',
              top: clusterCrop ? `-${clusterCrop.topPct}%` : '0',
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
