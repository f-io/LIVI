import React, { useCallback, useEffect, useMemo, useRef, useState, useLayoutEffect } from 'react'
import { Box, Typography, useTheme, alpha } from '@mui/material'
import { keyframes } from '@mui/system'
import { useLocation, useNavigate } from 'react-router-dom'
import { CommandMapping } from '@main/carplay/messages/common'

import { ExtraConfig } from '@main/Globals'
import { useCarplayStore, useStatusStore } from '../../../store/store'
import { InitEvent, Renderer } from '@worker/render/RenderEvents'
import type { CarPlayWorker, UsbEvent, KeyCommand, WorkerToUI, AudioData } from '@worker/types'
import useCarplayAudio from './hooks/useCarplayAudio'
import { useCarplayMultiTouch } from './hooks/useCarplayTouch'

// Icons
import UsbOffOutlinedIcon from '@mui/icons-material/UsbOffOutlined'
import UsbOutlinedIcon from '@mui/icons-material/UsbOutlined'
import PhoneIphoneOutlinedIcon from '@mui/icons-material/PhoneIphoneOutlined'

const RETRY_DELAY_MS = 3000

interface CarplayProps {
  receivingVideo: boolean
  setReceivingVideo: (v: boolean) => void
  settings: ExtraConfig
  command: KeyCommand
  commandCounter: number
}

/* ----------------------------- Overlay visuals ----------------------------- */

const spin = keyframes`to { transform: rotate(360deg); }`
const pulse = keyframes`
  0%   { transform: scale(1);   opacity: .35; }
  50%  { transform: scale(1.08); opacity: .7; }
  100% { transform: scale(1);   opacity: .35; }
`

function StatusOverlay({
  mode,
  show,
  offsetY = 0
}: {
  mode: 'dongle' | 'phone'
  show: boolean
  offsetY?: number
}) {
  const theme = useTheme()
  const isPhonePhase = mode === 'phone'
  const ringColor = isPhonePhase ? theme.palette.primary.main : theme.palette.text.secondary
  const track = alpha(ringColor, 0.22)

  // Measure ring size
  const ringRef = useRef<HTMLDivElement>(null)
  const [ringH, setRingH] = useState(0)
  useLayoutEffect(() => {
    const measure = () => {
      const h = ringRef.current?.getBoundingClientRect().height ?? 0
      if (h && h !== ringH) setRingH(h)
    }
    measure()
    const ro = ringRef.current ? new ResizeObserver(measure) : null
    if (ringRef.current) ro?.observe(ringRef.current)
    window.addEventListener('resize', measure)
    return () => {
      window.removeEventListener('resize', measure)
      ro?.disconnect()
    }
  }, [ringH])

  const GAP_BELOW = 12

  const Chip = (active: boolean, Icon: React.ElementType, label: string, muted?: boolean) => (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 0.75,
        px: 1.25,
        py: 0.5,
        borderRadius: 999,
        backdropFilter: 'blur(6px)',
        backgroundColor: alpha(
          theme.palette.background.paper,
          theme.palette.mode === 'dark' ? 0.28 : 0.18
        ),
        color: active
          ? theme.palette.primary.main
          : muted
            ? theme.palette.text.disabled
            : theme.palette.text.secondary,
        fontSize: 12,
        lineHeight: 1,
        border: `1px solid ${alpha(theme.palette.divider, 0.5)}`
      }}
    >
      <Icon sx={{ fontSize: 18 }} />
      <Typography variant="caption" sx={{ fontWeight: 500 }}>
        {label}
      </Typography>
    </Box>
  )

  return (
    <Box
      role="status"
      aria-live="polite"
      aria-hidden={!show}
      sx={{
        position: 'absolute',
        inset: 0,
        pointerEvents: 'none',
        display: show ? 'block' : 'none',
        zIndex: 9
      }}
    >
      {/* Ring center pinned to window center */}
      <Box
        ref={ringRef}
        sx={{
          position: 'absolute',
          left: '50%',
          top: `calc(50% + ${offsetY}px)`,
          transform: 'translate(-50%, -50%)',
          width: { xs: 72, sm: 88 },
          height: { xs: 72, sm: 88 }
        }}
      >
        {/* Track */}
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            borderRadius: '50%',
            border: '6px solid',
            borderColor: track
          }}
        />
        {/* Spinning arc */}
        <Box
          sx={{
            position: 'absolute',
            inset: 0,
            borderRadius: '50%',
            border: '6px solid',
            borderColor: 'transparent',
            borderTopColor: ringColor,
            animation: `${spin} 900ms linear infinite`
          }}
        />
        {/* Soft pulse */}
        <Box
          sx={{
            position: 'absolute',
            inset: 10,
            borderRadius: '50%',
            background: alpha(ringColor, 0.15),
            animation: `${pulse} 1400ms ease-in-out infinite`
          }}
        />
      </Box>

      {/* Chips */}
      <Box
        sx={{
          position: 'absolute',
          left: '50%',
          top: `calc(50% + ${offsetY}px + ${(ringH || 0) / 2 + GAP_BELOW}px)`,
          transform: 'translateX(-50%)',
          display: 'flex',
          alignItems: 'center',
          gap: 1.5
        }}
      >
        {isPhonePhase
          ? Chip(true, UsbOutlinedIcon, 'Dongle')
          : Chip(false, UsbOffOutlinedIcon, 'Dongle')}
        <Box sx={{ width: 18, height: 2, bgcolor: alpha(ringColor, 0.25), borderRadius: 1 }} />
        {Chip(false, PhoneIphoneOutlinedIcon, 'Phone', !isPhonePhase)}
      </Box>
    </Box>
  )
}

/* --------------------------------- Carplay -------------------------------- */

const CarplayComponent: React.FC<CarplayProps> = ({
  receivingVideo,
  setReceivingVideo,
  settings,
  command,
  commandCounter
}) => {
  const navigate = useNavigate()
  const location = useLocation()
  const pathname = location.pathname
  const theme = useTheme()

  // Zustand store
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const setStreaming = useStatusStore((s) => s.setStreaming)
  const setDongleConnected = useStatusStore((s) => s.setDongleConnected)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const resetInfo = useCarplayStore((s) => s.resetInfo)
  const setDeviceInfo = useCarplayStore((s) => s.setDeviceInfo)
  const setNegotiatedResolution = useCarplayStore((s) => s.setNegotiatedResolution)
  const setAudioInfo = useCarplayStore((s) => s.setAudioInfo)
  const setPcmData = useCarplayStore((s) => s.setPcmData)

  useEffect(() => {
    console.log('[UI] Dongle connected:', isDongleConnected)
  }, [isDongleConnected])

  // Refs
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const mainElem = useRef<HTMLDivElement>(null)
  const videoContainerRef = useRef<HTMLDivElement>(null)
  const retryTimeoutRef = useRef<NodeJS.Timeout | null>(null)
  const hasStartedRef = useRef(false)
  const [renderReady, setRenderReady] = useState(false)

  // Overlay offset
  const [overlayY, setOverlayY] = useState(0)
  useLayoutEffect(() => {
    const recalc = () => {
      const r = mainElem.current?.getBoundingClientRect()
      if (!r) return
      const contentCenterY = r.top + r.height / 2
      const windowCenterY = window.innerHeight / 2
      setOverlayY(windowCenterY - contentCenterY)
    }
    recalc()
    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(recalc) : null
    if (ro && mainElem.current) ro.observe(mainElem.current)
    window.addEventListener('resize', recalc)
    return () => {
      window.removeEventListener('resize', recalc)
      ro?.disconnect()
    }
  }, [])

  // MediaPlayStatus handling
  const mediaPlayStatusRef = useRef<number | undefined>(undefined)
  const audioCommandRef = useRef<number | undefined>(undefined)

  // Render worker + OffscreenCanvas
  const renderWorkerRef = useRef<Worker | null>(null)
  const offscreenCanvasRef = useRef<OffscreenCanvas | null>(null)

  // Render settings
  const preferredRenderer = 'auto' // 'auto' | 'webgl2' | 'webgl' | 'webgpu'
  const reportFps = false
  const useHardware = true
  const useWebRTC = true

  // Keep settings in ref
  const configRef = useRef(settings)
  useEffect(() => {
    configRef.current = settings
  }, [settings])

  // Channels
  const videoChannel = useMemo(() => new MessageChannel(), [])
  const audioChannel = useMemo(() => new MessageChannel(), [])

  // CarPlay worker setup
  const carplayWorker = useMemo<CarPlayWorker>(() => {
    const w = new Worker(new URL('../../worker/CarPlay.worker.ts', import.meta.url), {
      type: 'module'
    }) as CarPlayWorker

    w.onerror = (e) => {
      console.error('Worker error:', e)
    }

    console.log('[CARPLAY] Creating CarPlayWorker with port:', {
      audioPort: audioChannel.port1
    })

    w.postMessage(
      {
        type: 'initialise',
        payload: {
          audioPort: audioChannel.port1
        }
      },
      [audioChannel.port1]
    )
    return w
  }, [audioChannel])

  // Render worker setup
  useEffect(() => {
    if (canvasRef.current && !offscreenCanvasRef.current && !renderWorkerRef.current) {
      offscreenCanvasRef.current = canvasRef.current.transferControlToOffscreen()
      const w = new Worker(new URL('../../worker/render/Render.worker.ts', import.meta.url), {
        type: 'module'
      })
      renderWorkerRef.current = w
      w.postMessage(
        new InitEvent(
          offscreenCanvasRef.current,
          videoChannel.port2,
          preferredRenderer as Renderer,
          reportFps,
          useHardware,
          useWebRTC
        ),
        [offscreenCanvasRef.current, videoChannel.port2]
      )
    }
    // Cleanup when canvas is unmounted
    return () => {
      renderWorkerRef.current?.terminate()
      renderWorkerRef.current = null
      offscreenCanvasRef.current = null
    }
  }, [videoChannel, preferredRenderer, reportFps, useHardware, useWebRTC])

  useEffect(() => {
    if (!renderWorkerRef.current) return
    const handler = (ev: MessageEvent<{ type: 'render-ready' }>) => {
      if (ev.data?.type === 'render-ready') {
        console.log('[CARPLAY] Render worker ready message recived')
        setRenderReady(true)
      }
    }
    renderWorkerRef.current.addEventListener('message', handler)
    return () => renderWorkerRef.current?.removeEventListener('message', handler)
  }, [])

  // Forward video chunks to worker port
  useEffect(() => {
    const handleVideo = (payload: unknown) => {
      if (!renderReady || !payload || typeof payload !== 'object') return
      const m = payload as { chunk?: { buffer?: ArrayBuffer } }
      const buf = m.chunk?.buffer
      if (!buf) return
      videoChannel.port1.postMessage(buf, [buf])
    }
    window.carplay.ipc.onVideoChunk(handleVideo)
    return () => {}
  }, [videoChannel, renderReady])

  // Forward audio chunks to audio channel
  useEffect(() => {
    const handleAudio = (payload: unknown) => {
      if (!payload || typeof payload !== 'object') return
      const m = payload as { chunk?: { buffer?: ArrayBuffer } } & Record<string, unknown>
      const buf = m.chunk?.buffer
      if (!buf) return

      const { chunk: _chunk, ...rest } = m
      audioChannel.port2.postMessage({ type: 'audio', buffer: buf, ...rest }, [buf])
    }
    window.carplay.ipc.onAudioChunk(handleAudio)
    return () => {}
  }, [audioChannel])

  // Start CarPlay service on mount
  useEffect(() => {
    ;(async () => {
      try {
        await window.carplay.ipc.start()
      } catch (err) {
        console.error('CarPlay start failed:', err)
      }
    })()
  }, [])

  // Audio + touch hooks
  const { processAudio, getAudioPlayer } = useCarplayAudio(carplayWorker)
  const touchHandlers = useCarplayMultiTouch()

  const clearRetryTimeout = useCallback(() => {
    if (retryTimeoutRef.current) {
      clearTimeout(retryTimeoutRef.current)
      retryTimeoutRef.current = null
    }
  }, [])

  const gotoHostUI = useCallback(() => {
    if (location.pathname !== '/media') {
      navigate('/media', { replace: true })
    }
  }, [location.pathname, navigate])

  // CarPlay worker messages
  useEffect(() => {
    if (!carplayWorker) return
    const handler = (ev: MessageEvent<WorkerToUI>) => {
      const msg = ev.data
      switch (msg.type) {
        case 'plugged':
          setDongleConnected(true)
          break

        case 'unplugged':
          hasStartedRef.current = false
          setDongleConnected(false)
          setStreaming(false)
          setReceivingVideo(false)
          resetInfo()
          break

        case 'requestBuffer': {
          clearRetryTimeout()
          const m = (msg as Extract<WorkerToUI, { type: 'requestBuffer' }>).message
          getAudioPlayer(m)
          break
        }

        case 'audio': {
          clearRetryTimeout()
          const m = (msg as Extract<WorkerToUI, { type: 'audio' }>).message
          const audioPayload: AudioData & { command?: number } = {
            ...m,
            command: audioCommandRef.current
          }
          processAudio(audioPayload)
          audioCommandRef.current = undefined
          break
        }

        case 'audioInfo':
          setAudioInfo((msg as Extract<WorkerToUI, { type: 'audioInfo' }>).payload)
          break

        case 'pcmData':
          setPcmData(new Float32Array((msg as Extract<WorkerToUI, { type: 'pcmData' }>).payload))
          break

        case 'command': {
          const val = (msg as Extract<WorkerToUI, { type: 'command' }>).message?.value
          if (val === CommandMapping.requestHostUI) gotoHostUI()
          break
        }

        case 'dongleInfo': {
          const p = (msg as Extract<WorkerToUI, { type: 'dongleInfo' }>).payload
          setDeviceInfo({
            serial: p.serial ?? '',
            manufacturer: p.manufacturer ?? '',
            product: p.product ?? '',
            fwVersion: p.fwVersion ?? ''
          })
          break
        }

        case 'resolution': {
          const r = (msg as Extract<WorkerToUI, { type: 'resolution' }>).payload
          setNegotiatedResolution(r.width, r.height)
          setStreaming(true)
          setReceivingVideo(true)
          hasStartedRef.current = true
          break
        }

        case 'failure':
          hasStartedRef.current = false
          if (!retryTimeoutRef.current) {
            retryTimeoutRef.current = setTimeout(() => window.location.reload(), RETRY_DELAY_MS)
          }
          break
      }
    }

    carplayWorker.addEventListener('message', handler)
    return () => carplayWorker.removeEventListener('message', handler)
  }, [
    carplayWorker,
    clearRetryTimeout,
    getAudioPlayer,
    processAudio,
    gotoHostUI,
    setDeviceInfo,
    setNegotiatedResolution,
    setAudioInfo,
    setPcmData,
    setDongleConnected,
    setStreaming,
    resetInfo,
    setReceivingVideo
  ])

  // USB events
  useEffect(() => {
    const onUsbConnect = async () => {
      if (!hasStartedRef.current) {
        resetInfo()
        setDongleConnected(true)
        hasStartedRef.current = true
        await window.carplay.ipc.start()
      }
    }
    const onUsbDisconnect = async () => {
      clearRetryTimeout()
      setReceivingVideo(false)
      setStreaming(false)
      setDongleConnected(false)
      hasStartedRef.current = false
      resetInfo()
      await window.carplay.ipc.stop()
      if (canvasRef.current) {
        canvasRef.current.style.width = '0'
        canvasRef.current.style.height = '0'
      }
    }
    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = args[0] as UsbEvent | undefined
      if (!data) return
      if (data.type === 'plugged') onUsbConnect()
      else if (data.type === 'unplugged') onUsbDisconnect()
    }

    window.carplay.usb.listenForEvents(usbHandler)
    ;(async () => {
      const last = await window.carplay.usb.getLastEvent()
      if (last) usbHandler(undefined, last as unknown)
    })()

    return () => {
      window.carplay.usb.unlistenForEvents?.(usbHandler)
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
      // @ts-ignore
      window.electron?.ipcRenderer.removeListener('usb-event', usbHandler)
    }
  }, [setReceivingVideo, setDongleConnected, setStreaming, clearRetryTimeout, navigate, resetInfo])

  // Settings/events from main
  useEffect(() => {
    const handler = (_evt: unknown, data: unknown) => {
      const d = (data ?? {}) as Record<string, unknown>
      const t = typeof d.type === 'string' ? d.type : undefined

      switch (t) {
        case 'resolution': {
          const payload = d.payload as { width?: number; height?: number } | undefined
          if (payload && typeof payload.width === 'number' && typeof payload.height === 'number') {
            useCarplayStore.setState({
              negotiatedWidth: payload.width,
              negotiatedHeight: payload.height
            })
            useStatusStore.setState({ isStreaming: true })
            setReceivingVideo(true)
          }
          break
        }
        case 'audioInfo': {
          const p = d.payload as
            | {
                codec?: string
                sampleRate?: number
                channels?: number
                bitDepth?: number
              }
            | undefined
          if (p) {
            useCarplayStore.setState({
              audioCodec: p.codec,
              audioSampleRate: p.sampleRate,
              audioChannels: p.channels,
              audioBitDepth: p.bitDepth
            })
          }
          break
        }
        case 'media': {
          const playStatus = (
            d as {
              payload?: { payload?: { media?: { MediaPlayStatus?: number } } }
            }
          ).payload?.payload?.media?.MediaPlayStatus
          const prevStatus = mediaPlayStatusRef.current
          if (typeof playStatus === 'number' && playStatus !== prevStatus) {
            mediaPlayStatusRef.current = playStatus
            audioCommandRef.current = playStatus
          }
          break
        }
        case 'plugged':
          useStatusStore.setState({ isDongleConnected: true })
          break
        case 'unplugged':
          useStatusStore.setState({ isDongleConnected: false, isStreaming: false })
          useCarplayStore.getState().resetInfo()
          break
        case 'command': {
          const value = (d as { message?: { value?: number } }).message?.value
          if (value === CommandMapping.requestHostUI) gotoHostUI()
          break
        }
      }
    }

    // subscribe
    window.carplay.ipc.onEvent(handler)

    // best-effort cleanup for legacy emitter shape
    return () => {
      const remove = (
        window as unknown as {
          electron?: {
            ipcRenderer?: { removeListener?: (ch: string, l: (...a: unknown[]) => void) => void }
          }
        }
      ).electron?.ipcRenderer?.removeListener
      if (typeof remove === 'function')
        remove('carplay-event', handler as (...args: unknown[]) => void)
    }
  }, [gotoHostUI, setReceivingVideo])

  // Resize observer => inform render worker
  useEffect(() => {
    if (!carplayWorker || !mainElem.current) return
    const obs = new ResizeObserver(() => carplayWorker.postMessage({ type: 'frame' }))
    obs.observe(mainElem.current)
    return () => obs.disconnect()
  }, [carplayWorker])

  // Key commands
  useEffect(() => {
    if (commandCounter) {
      window.carplay.ipc.sendKeyCommand(command)
    }
  }, [command, commandCounter])

  // Cleanup
  useEffect(() => {
    return () => {
      carplayWorker.terminate()
      renderWorkerRef.current?.terminate()
      renderWorkerRef.current = null
      offscreenCanvasRef.current = null
    }
  }, [carplayWorker])

  /* ---------------------- Force-hide video when not streaming --------------- */
  useEffect(() => {
    if (!isStreaming) {
      setReceivingVideo(false)
      if (canvasRef.current) {
        canvasRef.current.style.width = '0'
        canvasRef.current.style.height = '0'
      }
      renderWorkerRef.current?.postMessage({ type: 'clear' })
    }
  }, [isStreaming, setReceivingVideo])

  /* ------------------------------- UI binding ------------------------------ */

  const mode: 'dongle' | 'phone' = !isDongleConnected ? 'dongle' : 'phone'

  return (
    <div
      id="main"
      ref={mainElem}
      className="App"
      style={
        pathname === '/'
          ? { height: '100%', width: '100%', touchAction: 'none', position: 'relative' }
          : { display: 'none' }
      }
    >
      {/* Overlay (ring + icon chips) */}
      {pathname === '/' && (
        <StatusOverlay show={!isDongleConnected || !isStreaming} mode={mode} offsetY={overlayY} />
      )}

      <div
        id="videoContainer"
        ref={videoContainerRef}
        {...touchHandlers}
        style={{
          height: '100%',
          width: '100%',
          padding: 0,
          margin: 0,
          display: 'flex',
          touchAction: 'none',
          backgroundColor: receivingVideo ? 'transparent' : theme.palette.background.default,
          visibility: receivingVideo ? 'visible' : 'hidden',
          zIndex: receivingVideo ? 1 : -1,
          position: 'relative'
        }}
      >
        <canvas
          ref={canvasRef}
          id="video"
          style={{
            width: receivingVideo ? '100%' : '0',
            height: receivingVideo ? '100%' : '0',
            touchAction: 'none',
            userSelect: 'none',
            pointerEvents: 'none'
          }}
        />
      </div>
    </div>
  )
}

export const Carplay = React.memo(CarplayComponent)
