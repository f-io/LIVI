import React, { useCallback, useEffect, useMemo, useRef, useState, useLayoutEffect } from 'react'
import { Box, useTheme } from '@mui/material'
import { useLocation, useNavigate } from 'react-router'
import { CommandMapping } from '@main/services/carplay/messages/common'
import { AudioCommand } from '@main/services/carplay/messages/readable'
import { ExtraConfig } from '@main/Globals'
import { useCarplayStore, useStatusStore } from '../../../store/store'
import { InitEvent, UpdateFpsEvent } from '@worker/render/RenderEvents'
import type { CarPlayWorker, UsbEvent, KeyCommand, WorkerToUI } from '@worker/types'
import { useCarplayMultiTouch } from './hooks/useCarplayTouch'

// Icons
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'

const RETRY_DELAY_MS = 3000

interface CarplayProps {
  receivingVideo: boolean
  setReceivingVideo: (v: boolean) => void
  settings: ExtraConfig
  command: KeyCommand
  commandCounter: number

  navVideoOverlayActive: boolean
  setNavVideoOverlayActive: (v: boolean) => void
}

function StatusOverlay({
  mode,
  show,
  offsetX = 0,
  offsetY = 0
}: {
  mode: 'dongle' | 'phone'
  show: boolean
  offsetX?: number
  offsetY?: number
}) {
  const theme = useTheme()
  const isPhonePhase = mode === 'phone'

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
      <Box
        sx={{
          position: 'absolute',
          left: `calc(50% + ${offsetX}px)`,
          top: `calc(50% + ${offsetY}px)`,
          transform: 'translate(-50%, -50%)',
          display: 'grid',
          placeItems: 'center'
        }}
      >
        <CropPortraitOutlinedIcon
          sx={{
            fontSize: 84,
            color: theme.palette.text.primary,
            opacity: isPhonePhase ? 'var(--ui-breathe-opacity, 1)' : 0.55
          }}
        />
      </Box>
    </Box>
  )
}

// Carplay

const CarplayComponent: React.FC<CarplayProps> = ({
  receivingVideo,
  setReceivingVideo,
  settings,
  command,
  commandCounter,
  navVideoOverlayActive,
  setNavVideoOverlayActive
}) => {
  const navigate = useNavigate()
  const location = useLocation()
  const pathname = location.pathname

  const pathnameRef = useRef(pathname)
  useEffect(() => {
    pathnameRef.current = pathname
  }, [pathname])

  const theme = useTheme()

  // Zustand store
  const isStreaming = useStatusStore((s) => s.isStreaming)
  const setStreaming = useStatusStore((s) => s.setStreaming)
  const setDongleConnected = useStatusStore((s) => s.setDongleConnected)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected)
  const resetInfo = useCarplayStore((s) => s.resetInfo)
  const setDeviceInfo = useCarplayStore((s) => s.setDeviceInfo)
  const setAudioInfo = useCarplayStore((s) => s.setAudioInfo)
  const setPcmData = useCarplayStore((s) => s.setPcmData)

  useEffect(() => {
    if (pathname !== '/') return
    void window.carplay.ipc.sendFrame().catch(() => {})
  }, [pathname])

  useEffect(() => {
    console.log('[CARPLAY] Dongle connected:', isDongleConnected)
  }, [isDongleConnected])

  // Refs
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const mainElem = useRef<HTMLDivElement>(null)
  const videoContainerRef = useRef<HTMLDivElement>(null)
  const retryTimeoutRef = useRef<NodeJS.Timeout | null>(null)
  const hasStartedRef = useRef(false)
  const [renderReady, setRenderReady] = useState(false)
  const [rendererError, setRendererError] = useState<string | null>(null)
  const lastNonCarplayPathRef = useRef<string | null>(null)
  const lastNonMapsPathRef = useRef<string | null>(null)
  const autoSwitchedRef = useRef(false)
  const pendingVideoFocusRef = useRef(false)

  const autoSwitchOnStreamRef = useRef(Boolean(settings.autoSwitchOnStream))
  const autoSwitchOnGuidanceRef = useRef(Boolean(settings.autoSwitchOnGuidance))
  const autoSwitchOnPhoneCallRef = useRef(Boolean(settings.autoSwitchOnPhoneCall))

  useEffect(() => {
    autoSwitchOnStreamRef.current = Boolean(settings.autoSwitchOnStream)
  }, [settings.autoSwitchOnStream])

  useEffect(() => {
    autoSwitchOnGuidanceRef.current = Boolean(settings.autoSwitchOnGuidance)
  }, [settings.autoSwitchOnGuidance])

  useEffect(() => {
    autoSwitchOnPhoneCallRef.current = Boolean(settings.autoSwitchOnPhoneCall)
  }, [settings.autoSwitchOnPhoneCall])

  // Attention-driven UI switching (call / siri / nav)
  type AttentionKind = 'call' | 'siri'
  type AttentionPayload = { kind: AttentionKind; active: boolean; phase?: string }

  const attentionBackPathRef = useRef<string | null>(null)
  const attentionSwitchedByRef = useRef<AttentionKind | null>(null)
  const siriReleaseTimerRef = useRef<number | null>(null)

  const clearSiriReleaseTimer = useCallback(() => {
    if (siriReleaseTimerRef.current != null) {
      window.clearTimeout(siriReleaseTimerRef.current)
      siriReleaseTimerRef.current = null
    }
  }, [])

  // Keep track of the last host UI route (anything except "/")
  useEffect(() => {
    if (pathname === '/') return
    if (!attentionSwitchedByRef.current) return

    attentionSwitchedByRef.current = null
    clearSiriReleaseTimer()
  }, [pathname, clearSiriReleaseTimer])

  useEffect(() => {
    // When NAV video overlay is shown on top of the host UI (not on "/")
    if (!navVideoOverlayActive || pathname === '/') return

    const dismiss = () => {
      setNavVideoOverlayActive(false)
    }

    // Any touch/click/pen should immediately dismiss
    window.addEventListener('pointerdown', dismiss, { capture: true, passive: true })

    return () => {
      window.removeEventListener('pointerdown', dismiss, {
        capture: true
      } as AddEventListenerOptions)
    }
  }, [navVideoOverlayActive, pathname, setNavVideoOverlayActive])

  // Overlay offset
  const [overlayX, setOverlayX] = useState(0)
  const [overlayY, setOverlayY] = useState(0)

  useLayoutEffect(() => {
    const getAnchor = () => document.getElementById('content-root')

    const recalc = () => {
      const r = getAnchor()?.getBoundingClientRect()
      if (!r) return

      const contentCenterX = r.left + r.width / 2
      const contentCenterY = r.top + r.height / 2

      const windowCenterX = window.innerWidth / 2
      const windowCenterY = window.innerHeight / 2

      setOverlayX(contentCenterX - windowCenterX)
      setOverlayY(contentCenterY - windowCenterY)
    }

    recalc()
    const raf = requestAnimationFrame(recalc)

    const ro = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(recalc) : null
    const anchor = getAnchor()
    if (ro && anchor) ro.observe(anchor)

    window.addEventListener('resize', recalc)
    return () => {
      cancelAnimationFrame(raf)
      window.removeEventListener('resize', recalc)
      ro?.disconnect()
    }
  }, [settings?.hand])

  // Render worker + OffscreenCanvas
  const renderWorkerRef = useRef<Worker | null>(null)
  const offscreenCanvasRef = useRef<OffscreenCanvas | null>(null)

  // keep initial FPS for worker init
  const initialFpsRef = useRef(settings.fps)

  // Visual delay for FFT so spectrum matches audio playback
  const fftVisualDelayMs = 0

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

      const targetFps = initialFpsRef.current

      w.postMessage(new InitEvent(offscreenCanvasRef.current, videoChannel.port2, targetFps), [
        offscreenCanvasRef.current,
        videoChannel.port2
      ])
    }
    // Cleanup when canvas is unmounted
    return () => {
      renderWorkerRef.current?.terminate()
      renderWorkerRef.current = null
      offscreenCanvasRef.current = null
    }
  }, [videoChannel])

  useEffect(() => {
    if (!renderWorkerRef.current) return
    renderWorkerRef.current.postMessage(new UpdateFpsEvent(settings.fps))
  }, [settings.fps])

  useEffect(() => {
    const w = renderWorkerRef.current
    if (!w) return

    type RenderWorkerMsg =
      | { type: 'render-ready' }
      | { type: 'render-error'; message?: string }
      | { type: string; [key: string]: unknown }

    const isRecord = (v: unknown): v is Record<string, unknown> =>
      typeof v === 'object' && v !== null

    const readWorkerMsg = (data: unknown): RenderWorkerMsg | null => {
      if (!isRecord(data)) return null
      const t = data.type
      if (typeof t !== 'string') return null
      return data as RenderWorkerMsg
    }

    const handler = (ev: MessageEvent<unknown>) => {
      const msg = readWorkerMsg(ev.data)
      const t = msg?.type

      if (t === 'render-ready') {
        console.log('[CARPLAY] Render worker ready message received')
        setRenderReady(true)
        setRendererError(null)
        return
      }

      if (t === 'render-error') {
        const message = msg && typeof msg.message === 'string' ? msg.message.trim() : ''
        const text = message ? message : 'No renderer available'

        console.warn('[CARPLAY] Render worker error:', msg)

        setRendererError(text)
        setRenderReady(false)
        setReceivingVideo(false)
        w.postMessage({ type: 'clear' })
      }
    }

    w.addEventListener('message', handler)
    return () => w.removeEventListener('message', handler)
  }, [setReceivingVideo])

  // Forward video chunks to worker port
  useEffect(() => {
    const handleVideo = (payload: unknown) => {
      if (rendererError) return
      if (!renderReady || !payload || typeof payload !== 'object') return
      const m = payload as { chunk?: { buffer?: ArrayBuffer } }
      const buf = m.chunk?.buffer
      if (!buf) return
      videoChannel.port1.postMessage(buf, [buf])
    }
    window.carplay.ipc.onVideoChunk(handleVideo)
    return () => {}
  }, [videoChannel, renderReady, rendererError])

  // Forward audio chunks to FFT
  useEffect(() => {
    const timers = new Set<number>()

    const handleAudio = (payload: unknown) => {
      if (!payload || typeof payload !== 'object') return

      const m = payload as { chunk?: { buffer?: ArrayBuffer } } & Record<string, unknown>
      const buf = m.chunk?.buffer
      if (!buf) return

      // mono Int16 from main -> Float32 [-1, 1] for FFT
      const int16 = new Int16Array(buf)
      const f32 = new Float32Array(int16.length)
      for (let i = 0; i < int16.length; i += 1) {
        f32[i] = int16[i] / 32768
      }

      const id = window.setTimeout(() => {
        timers.delete(id)
        setPcmData(f32)
      }, fftVisualDelayMs)
      timers.add(id)
    }

    window.carplay.ipc.onAudioChunk(handleAudio)

    return () => {
      for (const id of timers) {
        window.clearTimeout(id)
      }
      timers.clear()
    }
  }, [setPcmData, fftVisualDelayMs])

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
  const touchHandlers = useCarplayMultiTouch()

  const clearRetryTimeout = useCallback(() => {
    if (retryTimeoutRef.current) {
      clearTimeout(retryTimeoutRef.current)
      retryTimeoutRef.current = null
    }
  }, [])

  const gotoHostUI = useCallback(() => {
    const target = settings.mapsEnabled ? '/maps' : '/media'
    if (location.pathname !== target) {
      navigate(target, { replace: true })
    }
  }, [location.pathname, navigate, settings.mapsEnabled])

  const applyAttention = useCallback(
    (p: AttentionPayload) => {
      const inCarplay = location.pathname === '/'

      if (p.kind !== 'call' && p.kind !== 'siri') return

      // ACTIVE: switch to CarPlay
      if (p.active) {
        if (p.kind === 'siri') clearSiriReleaseTimer()

        // Already on CarPlay -> nothing to do
        if (inCarplay) {
          attentionSwitchedByRef.current = null
          return
        }

        // Not on CarPlay -> we will switch now, so arm return
        attentionBackPathRef.current = location.pathname
        attentionSwitchedByRef.current = p.kind

        navigate('/', { replace: true })
        return
      }

      // INACTIVE: only return if we previously switched because of this kind
      if (attentionSwitchedByRef.current !== p.kind) return

      const back = attentionBackPathRef.current

      const doReturn = () => {
        attentionSwitchedByRef.current = null
        if (back && back !== '/' && location.pathname === '/') {
          navigate(back, { replace: true })
        }
      }

      // Siri: debounce return to avoid flicker
      if (p.kind === 'siri') {
        clearSiriReleaseTimer()
        siriReleaseTimerRef.current = window.setTimeout(() => {
          siriReleaseTimerRef.current = null

          if (attentionSwitchedByRef.current !== 'siri') return

          doReturn()
        }, 120)

        return
      }

      // Call: return immediately
      doReturn()
    },
    [location.pathname, navigate, clearSiriReleaseTimer]
  )

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
          setNavVideoOverlayActive(false)
          break

        case 'requestBuffer': {
          clearRetryTimeout()
          break
        }

        case 'audio': {
          clearRetryTimeout()
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
    gotoHostUI,
    setDeviceInfo,
    setAudioInfo,
    setPcmData,
    setDongleConnected,
    setStreaming,
    resetInfo,
    setNavVideoOverlayActive,
    setReceivingVideo
  ])

  // USB events
  useEffect(() => {
    const onUsbConnect = async () => {
      if (!hasStartedRef.current) {
        resetInfo()
        try {
          const info = await window.carplay.usb.getDeviceInfo()
          if (info?.device) {
            setDeviceInfo({
              vendorId: info.vendorId,
              productId: info.productId,
              usbFwVersion: info.usbFwVersion ?? ''
            })
          }
        } catch (e) {
          console.warn('[CARPLAY] usb.getDeviceInfo() failed', e)
        }

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
  }, [
    setReceivingVideo,
    setDongleConnected,
    setStreaming,
    clearRetryTimeout,
    navigate,
    resetInfo,
    setDeviceInfo
  ])

  // Settings/events from main
  useEffect(() => {
    const mergeBoxInfo = (prev: unknown, next: unknown): unknown => {
      if (next == null) return prev
      if (typeof next === 'string') {
        const s = next.trim()
        if (!s) return prev
        try {
          next = JSON.parse(s)
        } catch {
          return prev
        }
      }
      if (typeof prev === 'string') {
        const s = prev.trim()
        if (s) {
          try {
            prev = JSON.parse(s)
          } catch {
            prev = null
          }
        } else {
          prev = null
        }
      }
      const isRecord = (v: unknown): v is Record<string, unknown> =>
        typeof v === 'object' && v !== null

      if (isRecord(prev) && isRecord(next)) {
        return { ...prev, ...next }
      }
      return next
    }

    const handler = (_evt: unknown, data: unknown) => {
      const pathnameNow = pathnameRef.current

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
            if (!rendererError) {
              setReceivingVideo(true)
            }

            if (pendingVideoFocusRef.current) {
              pendingVideoFocusRef.current = false
              if (pathnameNow !== '/') {
                navigate('/', { replace: true })
              }
            }
          }
          break
        }

        case 'dongleInfo': {
          const p = d.payload as { dongleFwVersion?: string; boxInfo?: unknown } | undefined
          if (!p) break
          useCarplayStore.setState((s) => ({
            dongleFwVersion: p.dongleFwVersion ?? s.dongleFwVersion,
            boxInfo: mergeBoxInfo(s.boxInfo, p.boxInfo)
          }))
          break
        }

        case 'audio': {
          const cmd = (d as { payload?: { command?: number } }).payload?.command
          if (typeof cmd !== 'number') break

          if (cmd === AudioCommand.AudioPhonecallStart) {
            if (autoSwitchOnPhoneCallRef.current) {
              applyAttention({ kind: 'call', active: true, phase: 'active' })
            }
          } else if (cmd === AudioCommand.AudioPhonecallStop) {
            applyAttention({ kind: 'call', active: false, phase: 'ended' })
          } else if (cmd === AudioCommand.AudioAttentionRinging) {
            if (autoSwitchOnPhoneCallRef.current) {
              applyAttention({ kind: 'call', active: true, phase: 'ringing' })
            }
          } else if (cmd === AudioCommand.AudioSiriStart) {
            applyAttention({ kind: 'siri', active: true })
          } else if (cmd === AudioCommand.AudioSiriStop) {
            applyAttention({ kind: 'siri', active: false })
          }
          break
        }

        case 'command': {
          const value = (d as { message?: { value?: number } }).message?.value
          if (typeof value !== 'number') break

          if (value === CommandMapping.requestHostUI) {
            gotoHostUI()
            break
          }

          const mapsEnabled = Boolean(settings.mapsEnabled)
          const autoSwitchOnStream = autoSwitchOnStreamRef.current
          const autoSwitchOnGuidance = autoSwitchOnGuidanceRef.current

          if (value === CommandMapping.naviFocus) {
            if (!autoSwitchOnGuidance) break

            if (mapsEnabled) {
              if (pathnameNow === '/' || pathnameNow === '/maps') break

              lastNonMapsPathRef.current = pathnameNow
              navigate('/maps', { replace: true })
              break
            }

            if (pathnameNow !== '/') {
              setNavVideoOverlayActive(true)
            }
            break
          }

          if (value === CommandMapping.naviRelease) {
            if (!autoSwitchOnGuidance) break
            if (mapsEnabled) {
              const back = lastNonMapsPathRef.current
              if (back && back !== '/maps' && back !== '/') {
                lastNonMapsPathRef.current = null
                navigate(back, { replace: true })
              }
              break
            }

            setNavVideoOverlayActive(false)
            break
          }

          if (value === CommandMapping.requestVideoFocus) {
            if (!autoSwitchOnStream) break
            if (attentionSwitchedByRef.current) break

            if (pathnameNow !== '/' && pathnameNow !== '/maps') {
              lastNonCarplayPathRef.current = pathnameNow
              autoSwitchedRef.current = true
            }

            if (!isStreaming) {
              pendingVideoFocusRef.current = true
              break
            }

            if (pathnameNow !== '/') {
              navigate('/', { replace: true })
            }
            break
          }

          if (value === CommandMapping.releaseVideoFocus) {
            if (!autoSwitchOnStream) {
              pendingVideoFocusRef.current = false
              autoSwitchedRef.current = false
              lastNonCarplayPathRef.current = null
              break
            }

            const backFromMaps = lastNonMapsPathRef.current

            if (
              mapsEnabled &&
              pathnameNow === '/maps' &&
              backFromMaps &&
              backFromMaps !== '/maps' &&
              backFromMaps !== '/'
            ) {
              lastNonMapsPathRef.current = null
              navigate(backFromMaps, { replace: true })
              break
            }

            if (attentionSwitchedByRef.current) {
              autoSwitchedRef.current = false
              lastNonCarplayPathRef.current = null
              break
            }

            if (autoSwitchedRef.current && lastNonCarplayPathRef.current) {
              navigate(lastNonCarplayPathRef.current, { replace: true })
            }
            autoSwitchedRef.current = false
            lastNonCarplayPathRef.current = null
            break
          }
          break
        }
      }
    }

    window.carplay.ipc.onEvent(handler)
    return () => window.carplay.ipc.offEvent(handler)
  }, [
    gotoHostUI,
    setReceivingVideo,
    navigate,
    isStreaming,
    setNavVideoOverlayActive,
    applyAttention,
    rendererError,
    settings.mapsEnabled
  ])

  // Resize observer => inform render worker
  useEffect(() => {
    if (!carplayWorker || !mainElem.current) return
    const obs = new ResizeObserver(() => carplayWorker.postMessage({ type: 'frame' }))
    obs.observe(mainElem.current)
    return () => obs.disconnect()
  }, [carplayWorker])

  // Key commands
  useEffect(() => {
    if (!commandCounter) return
    if (!isStreaming) return

    window.carplay.ipc.sendKeyCommand(command)
  }, [command, commandCounter, isStreaming])

  // Cleanup
  useEffect(() => {
    return () => {
      if (retryTimeoutRef.current) {
        clearTimeout(retryTimeoutRef.current)
        retryTimeoutRef.current = null
      }

      carplayWorker.terminate()
    }
  }, [carplayWorker])

  // Force-hide video when not streaming
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

  const inCarplay = pathname === '/'
  const showCarplayOverlay = inCarplay || navVideoOverlayActive

  return (
    <div
      id="carplay-root"
      ref={mainElem}
      style={{
        position: 'fixed',
        inset: 0,
        width: '100%',
        height: '100%',
        touchAction: 'none',

        // show as full UI when on "/" OR as overlay when navFocusActive
        visibility: showCarplayOverlay ? 'visible' : 'hidden',
        opacity: showCarplayOverlay ? 1 : 0,
        transition: 'opacity 120ms ease',

        // IMPORTANT:
        // - In CarPlay route: allow touch (so touchHandlers work)
        // - In overlay mode: do NOT swallow clicks/touch/keys for the host UI
        pointerEvents: inCarplay && isStreaming ? 'auto' : 'none',

        zIndex: showCarplayOverlay ? 999 : -1
      }}
    >
      {/* Overlay (ring + icon chips) */}
      {pathname === '/' && (
        <StatusOverlay
          show={!isDongleConnected || !isStreaming}
          mode={mode}
          offsetX={overlayX}
          offsetY={overlayY}
        />
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
          backgroundColor:
            receivingVideo && !rendererError ? 'transparent' : theme.palette.background.default,
          visibility: receivingVideo && !rendererError ? 'visible' : 'hidden',
          zIndex: receivingVideo && !rendererError ? 1 : -1,
          position: 'relative'
        }}
      >
        <canvas
          ref={canvasRef}
          id="video"
          style={{
            width: receivingVideo && !rendererError ? '100%' : '0',
            height: receivingVideo && !rendererError ? '100%' : '0',
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
