// Icons
import CropPortraitOutlinedIcon from '@mui/icons-material/CropPortraitOutlined'
import { Box, useTheme } from '@mui/material'
import type { Config } from '@shared/types'
import { PhoneType } from '@shared/types/Config'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import { aaContentArea, isClusterDisplayed } from '@shared/utils'
import { createProjectionWorker } from '@worker/createProjectionWorker'
import type { KeyCommand, ProjectionWorker, UsbEvent, WorkerToUI } from '@worker/types'
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { useFftPcm } from '../../../hooks/useFftPcm'
import { useLiviStore, useStatusStore } from '../../../store/store'
import { useProjectionMultiTouch } from './hooks/useProjectionTouch'

const RETRY_DELAY_MS = 3000

interface CarplayProps {
  receivingVideo: boolean
  setReceivingVideo: (v: boolean) => void
  settings: Config
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

// Projection

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
  const setAaActive = useStatusStore((s) => s.setAaActive)
  const isDongleConnected = useStatusStore((s) => s.isDongleConnected || s.isAaActive)
  const resetInfo = useLiviStore((s) => s.resetInfo)
  const setDeviceInfo = useLiviStore((s) => s.setDeviceInfo)
  const setAudioInfo = useLiviStore((s) => s.setAudioInfo)
  const setPcmData = useLiviStore((s) => s.setPcmData)
  const setBluetoothPairedList = useLiviStore((s) => s.setBluetoothPairedList)
  const bumpAudioDevicesRevision = useLiviStore((s) => s.bumpAudioDevicesRevision)
  const isAaActiveFlag = useStatusStore((s) => s.isAaActive)
  const negotiatedWidth = useLiviStore((s) => s.negotiatedWidth)
  const negotiatedHeight = useLiviStore((s) => s.negotiatedHeight)
  const wirelessAaEnabled = useLiviStore((s) => Boolean(s.settings?.wirelessAaEnabled))

  const prevPathnameRef = useRef(pathname)
  useEffect(() => {
    const prev = prevPathnameRef.current
    prevPathnameRef.current = pathname
    if (pathname !== '/' || prev === '/') return
    if (!isDongleConnected) return
    window.projection.ipc.sendCommand('home')
    void window.projection.ipc.sendFrame().catch(() => {})
  }, [pathname, isDongleConnected])

  // Tell main when the projection surface is shown/hidden so the native
  // GStreamer video can be shown over the UI or hidden behind it
  useEffect(() => {
    const visible = pathname === '/' || navVideoOverlayActive
    void window.projection.ipc.setVisible(visible).catch(() => {})
    document.documentElement.classList.toggle('show-video', visible && receivingVideo)
  }, [pathname, navVideoOverlayActive, receivingVideo])

  useEffect(() => {
    const mode = isAaActiveFlag ? 'AA' : 'dongle'
    console.log(`[PROJECTION] phone connected (${mode}):`, isDongleConnected)
  }, [isDongleConnected, isAaActiveFlag])

  // Refs
  const mainElem = useRef<HTMLDivElement>(null)
  const videoContainerRef = useRef<HTMLDivElement>(null)
  const retryTimeoutRef = useRef<NodeJS.Timeout | null>(null)
  const usbOpTokenRef = useRef(0)
  const hasStartedRef = useRef(false)
  const [rendererError] = useState<string | null>(null)
  const lastNonCarplayPathRef = useRef<string | null>(null)
  const lastNonClusterPathRef = useRef<string | null>(null)
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

  // Attention-driven UI switching (call / voiceAssistant / nav)
  type AttentionKind = 'call' | 'voiceAssistant'
  type AttentionPayload = { kind: AttentionKind; active: boolean; phase?: string }

  const attentionBackPathRef = useRef<string | null>(null)
  const attentionSwitchedByRef = useRef<AttentionKind | null>(null)
  const voiceAssistantReleaseTimerRef = useRef<number | null>(null)

  const clearVoiceAssistantReleaseTimer = useCallback(() => {
    if (voiceAssistantReleaseTimerRef.current != null) {
      window.clearTimeout(voiceAssistantReleaseTimerRef.current)
      voiceAssistantReleaseTimerRef.current = null
    }
  }, [])

  // Keep track of the last host UI route (anything except "/")
  useEffect(() => {
    if (pathname === '/') return
    if (!attentionSwitchedByRef.current) return

    attentionSwitchedByRef.current = null
    clearVoiceAssistantReleaseTimer()
  }, [pathname, clearVoiceAssistantReleaseTimer])

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

  // Visual delay for FFT so spectrum matches audio playback
  const fftVisualDelayMs = 0

  // Channels
  const audioChannel = useMemo(() => new MessageChannel(), [])

  // Projection worker setup
  const carplayWorker = useMemo<ProjectionWorker>(() => {
    const w = createProjectionWorker()

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

  // Forward audio chunks to FFT (shared with the secondary windows via useFftPcm)
  useFftPcm(fftVisualDelayMs)

  // Audio + touch hooks

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

  const applyAttention = useCallback(
    (p: AttentionPayload) => {
      const inProjection = location.pathname === '/'

      if (p.kind !== 'call' && p.kind !== 'voiceAssistant') return

      // ACTIVE: switch to projection
      if (p.active) {
        if (p.kind === 'voiceAssistant') clearVoiceAssistantReleaseTimer()

        // Already on projection -> nothing to do
        if (inProjection) {
          attentionSwitchedByRef.current = null
          return
        }

        // Not on projection -> we will switch now, so arm return
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

      // Voice assistant: debounce return to avoid flicker
      if (p.kind === 'voiceAssistant') {
        clearVoiceAssistantReleaseTimer()
        voiceAssistantReleaseTimerRef.current = window.setTimeout(() => {
          voiceAssistantReleaseTimerRef.current = null

          if (attentionSwitchedByRef.current !== 'voiceAssistant') return

          doReturn()
        }, 120)

        return
      }

      // Call: return immediately
      doReturn()
    },
    [location.pathname, navigate, clearVoiceAssistantReleaseTimer]
  )

  // Projection worker messages
  useEffect(() => {
    if (!carplayWorker) return
    const handler = (ev: MessageEvent<WorkerToUI>) => {
      const msg = ev.data
      switch (msg.type) {
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
    setReceivingVideo
  ])

  // USB events
  useEffect(() => {
    let disposed = false

    const onUsbConnect = async () => {
      const token = ++usbOpTokenRef.current
      if (!hasStartedRef.current) {
        resetInfo()

        let info:
          | { device: false; vendorId: null; productId: null; usbFwVersion: string }
          | { device: true; vendorId: number; productId: number; usbFwVersion: string }
          | null = null

        try {
          info = await window.projection.usb.getDeviceInfo()
        } catch (e) {
          console.warn('[PROJECTION] usb.getDeviceInfo() failed', e)
        }

        if (disposed || token !== usbOpTokenRef.current) return

        if (info?.device) {
          setDeviceInfo({
            vendorId: info.vendorId,
            productId: info.productId,
            usbFwVersion: info.usbFwVersion ?? ''
          })
        }

        setDongleConnected(true)
        hasStartedRef.current = true
      }
    }

    const onUsbDisconnect = async () => {
      usbOpTokenRef.current += 1
      clearRetryTimeout()
      setReceivingVideo(false)
      setStreaming(false)
      setDongleConnected(false)
      hasStartedRef.current = false
      resetInfo()
      await window.projection.ipc.stop()
    }
    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = args[0] as UsbEvent | undefined
      if (!data) return
      if (data.type === 'plugged') onUsbConnect()
      else if (data.type === 'unplugged') onUsbDisconnect()
    }

    const unsubscribe = window.projection.usb.listenForEvents(usbHandler)

    return () => {
      disposed = true
      unsubscribe?.()
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
        case 'bluetoothPairedList': {
          const raw =
            typeof d.payload === 'string'
              ? d.payload
              : typeof (d.payload as { data?: unknown } | undefined)?.data === 'string'
                ? ((d.payload as { data?: string }).data as string)
                : typeof d.data === 'string'
                  ? (d.data as string)
                  : ''

          setBluetoothPairedList(raw)
          break
        }
        case 'audioDevicesChanged': {
          bumpAudioDevicesRevision()
          break
        }
        case 'resolution': {
          const payload = d.payload as { width?: number; height?: number } | undefined
          if (payload && typeof payload.width === 'number' && typeof payload.height === 'number') {
            useLiviStore.setState({
              negotiatedWidth: payload.width,
              negotiatedHeight: payload.height
            })

            if (!rendererError) {
              setReceivingVideo(true)
              setStreaming(true)
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
          useLiviStore.setState((s) => ({
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
          } else if (cmd === AudioCommand.AudioVoiceAssistantStart) {
            applyAttention({ kind: 'voiceAssistant', active: true })
          } else if (cmd === AudioCommand.AudioVoiceAssistantStop) {
            applyAttention({ kind: 'voiceAssistant', active: false })
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

          if (!p) break

          setAudioInfo({
            codec: p.codec ?? '',
            sampleRate: p.sampleRate ?? 0,
            channels: p.channels ?? 0,
            bitDepth: p.bitDepth ?? 0
          })

          break
        }

        case 'command': {
          const value = (d as { message?: { value?: number } }).message?.value
          if (typeof value !== 'number') break

          if (value === CommandMapping.requestHostUI) {
            gotoHostUI()
            break
          }

          const clusterEnabled = isClusterDisplayed(settings)
          const autoSwitchOnStream = autoSwitchOnStreamRef.current
          const autoSwitchOnGuidance = autoSwitchOnGuidanceRef.current

          if (value === CommandMapping.requestClusterFocus) {
            if (!autoSwitchOnGuidance) break

            if (clusterEnabled) {
              if (pathnameNow === '/' || pathnameNow === '/cluster') break

              lastNonClusterPathRef.current = pathnameNow
              navigate('/cluster', { replace: true })
              break
            }

            if (pathnameNow !== '/') {
              setNavVideoOverlayActive(true)
            }
            break
          }

          if (value === CommandMapping.releaseClusterFocus) {
            if (!autoSwitchOnGuidance) break
            if (clusterEnabled) {
              const back = lastNonClusterPathRef.current
              if (back && back !== '/cluster' && back !== '/') {
                lastNonClusterPathRef.current = null
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

            if (pathnameNow !== '/' && pathnameNow !== '/cluster') {
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

            const backFromCluster = lastNonClusterPathRef.current

            if (
              clusterEnabled &&
              pathnameNow === '/cluster' &&
              backFromCluster &&
              backFromCluster !== '/cluster' &&
              backFromCluster !== '/'
            ) {
              lastNonClusterPathRef.current = null
              navigate(backFromCluster, { replace: true })
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

        case 'plugged': {
          const phoneType = (d as { phoneType?: number }).phoneType
          const useAa =
            phoneType !== undefined ? phoneType === PhoneType.AndroidAuto : wirelessAaEnabled
          if (useAa) setAaActive(true)
          else setDongleConnected(true)
          break
        }

        case 'unplugged': {
          setStreaming(false)
          setAaActive(false)
          setDongleConnected(false)
          setReceivingVideo(false)
          pendingVideoFocusRef.current = false
          setNavVideoOverlayActive(false)
          break
        }

        case 'failure': {
          setStreaming(false)
          setAaActive(false)
          setDongleConnected(false)
          setReceivingVideo(false)
          pendingVideoFocusRef.current = false
          setNavVideoOverlayActive(false)
          break
        }
      }
    }

    const unsubscribe = window.projection.ipc.onEvent(handler)
    return unsubscribe
  }, [
    gotoHostUI,
    setReceivingVideo,
    navigate,
    setStreaming,
    isStreaming,
    setDongleConnected,
    setNavVideoOverlayActive,
    applyAttention,
    rendererError,
    setAudioInfo,
    setBluetoothPairedList,
    bumpAudioDevicesRevision,
    settings.dashboards
  ])

  // Resize observer => inform render worker
  useEffect(() => {
    if (!carplayWorker || !mainElem.current) return
    const obs = new ResizeObserver(() => carplayWorker.postMessage({ type: 'frame' }))
    obs.observe(mainElem.current)
    return () => obs.disconnect()
  }, [carplayWorker])

  // Key commands. Fire only when the counter actually advances
  const lastSentCommandCounterRef = useRef(0)
  useEffect(() => {
    if (!commandCounter) return
    if (commandCounter === lastSentCommandCounterRef.current) return
    lastSentCommandCounterRef.current = commandCounter
    window.projection.ipc.sendCommand(command)
  }, [command, commandCounter])

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
    if (!isStreaming) setReceivingVideo(false)
  }, [isStreaming, setReceivingVideo])

  /* ------------------------------- UI binding ------------------------------ */

  const mode: 'dongle' | 'phone' = !isDongleConnected ? 'dongle' : 'phone'

  const inProjection = pathname === '/'
  const showProjectionOverlay = inProjection || navVideoOverlayActive

  const resolvedNegotiatedWidth = negotiatedWidth ?? 0
  const resolvedNegotiatedHeight = negotiatedHeight ?? 0

  // The phone renders a user-chosen AR inside the transport tier
  const aaContent =
    resolvedNegotiatedWidth > 0 &&
    resolvedNegotiatedHeight > 0 &&
    settings.width > 0 &&
    settings.height > 0
      ? aaContentArea(
          { width: resolvedNegotiatedWidth, height: resolvedNegotiatedHeight },
          { width: settings.width, height: settings.height }
        )
      : null

  const visibleWidth = aaContent?.contentWidth ?? resolvedNegotiatedWidth
  const visibleHeight = aaContent?.contentHeight ?? resolvedNegotiatedHeight

  const touchHandlers = useProjectionMultiTouch(
    videoContainerRef,
    resolvedNegotiatedWidth > 0 && resolvedNegotiatedHeight > 0
      ? {
          streamWidth: resolvedNegotiatedWidth,
          streamHeight: resolvedNegotiatedHeight,
          cropLeft: Math.max(0, (resolvedNegotiatedWidth - visibleWidth) / 2),
          cropTop: Math.max(0, (resolvedNegotiatedHeight - visibleHeight) / 2),
          visibleWidth,
          visibleHeight
        }
      : undefined
  )

  return (
    <div
      id="projection-root"
      ref={mainElem}
      style={{
        position: 'fixed',
        inset: 0,
        width: '100%',
        height: '100%',
        touchAction: 'none',
        visibility: showProjectionOverlay ? 'visible' : 'hidden',
        opacity: showProjectionOverlay ? 1 : 0,
        transition: 'opacity 120ms ease',
        pointerEvents: inProjection && isStreaming ? 'auto' : 'none',
        zIndex: showProjectionOverlay ? 999 : -1
      }}
    >
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
          display: 'block',
          touchAction: 'none',
          backgroundColor:
            receivingVideo && !rendererError ? 'transparent' : theme.palette.background.default,
          visibility: receivingVideo && !rendererError ? 'visible' : 'hidden',
          zIndex: receivingVideo && !rendererError ? 1 : -1,
          position: 'relative',
          overflow: 'hidden'
        }}
      />
    </div>
  )
}

export const Projection = React.memo(CarplayComponent)
