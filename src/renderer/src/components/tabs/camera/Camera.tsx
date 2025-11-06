import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Typography } from '@mui/material'

type CameraId = string

interface CameraProps {
  settings: { camera: CameraId } | null
  width?: number
  height?: number
  allowFallback?: boolean
  showFallbackNotice?: boolean
  withAudio?: boolean
}

type OpenStatus =
  | { state: 'idle' }
  | { state: 'opening' }
  | { state: 'ok'; exactMatched: boolean }
  | { state: 'error'; message: string }

export const Camera: React.FC<CameraProps> = ({
  settings,
  width = 800,
  height,
  allowFallback = true,
  showFallbackNotice = true,
  withAudio = false
}) => {
  const videoRef = useRef<HTMLVideoElement>(null)
  const currentStreamRef = useRef<MediaStream | null>(null)
  const [status, setStatus] = useState<OpenStatus>({ state: 'idle' })

  const savedId: CameraId | '' = useMemo(() => settings?.camera ?? '', [settings?.camera])

  const stopActiveStream = useCallback(() => {
    const stream = currentStreamRef.current
    if (stream) {
      try {
        stream.getTracks().forEach((t) => t.stop())
      } catch {}
      currentStreamRef.current = null
    }
    const videoEl = videoRef.current
    if (videoEl) {
      try {
        videoEl.pause()
      } catch {}
      try {
        ;(videoEl as HTMLVideoElement).srcObject = null
      } catch {}
    }
  }, [])

  const playVideo = useCallback(async (stream: MediaStream) => {
    const videoEl = videoRef.current
    if (!videoEl) return
    videoEl.srcObject = stream
    const p = videoEl.play()
    if (p && typeof (p as Promise<void>).catch === 'function') {
      ;(p as Promise<void>).catch(() => {})
    }
  }, [])

  // try getUserMedia with a list of constraint variants until one succeeds
  const tryOpenWithVariants = useCallback(
    async (variants: MediaStreamConstraints[], signal: AbortSignal) => {
      for (const constraints of variants) {
        if (signal.aborted) throw new Error('aborted')
        try {
          const s = await navigator.mediaDevices.getUserMedia(constraints)
          if (signal.aborted) {
            s.getTracks().forEach((t) => t.stop())
            throw new Error('aborted')
          }
          return s
        } catch (e) {
          // keep trying next variant
        }
      }
      throw new Error('all-variants-failed')
    },
    []
  )

  // Build constraint variants for a given deviceId
  const buildExactVariants = useCallback(
    (deviceId: string): MediaStreamConstraints[] => {
      const baseExact: MediaTrackConstraints = { deviceId: { exact: deviceId } }
      const sized: Array<[number, number]> = [
        // Common action-cam / UVC modes
        [1280, 720],
        [1920, 1080],
        [854, 480],
        [640, 480]
      ]

      const variants: MediaStreamConstraints[] = [{ video: baseExact, audio: withAudio }]

      if (width || height) {
        variants.push({
          video: {
            ...baseExact,
            width: width ? { ideal: width } : undefined,
            height: height ? { ideal: height } : undefined
          },
          audio: withAudio
        })
      }

      for (const [w, h] of sized) {
        variants.push({
          video: { ...baseExact, width: { exact: w }, height: { exact: h } },
          audio: withAudio
        })
        variants.push({
          video: { ...baseExact, width: { ideal: w }, height: { ideal: h } },
          audio: withAudio
        })
      }

      return variants
    },
    [width, height, withAudio]
  )

  const buildFallbackVariants = useCallback((): MediaStreamConstraints[] => {
    const sized: Array<[number, number]> = [
      [1280, 720],
      [1920, 1080],
      [854, 480],
      [640, 480]
    ]
    const variants: MediaStreamConstraints[] = [{ video: true, audio: withAudio }]
    if (width || height) {
      variants.push({
        video: {
          width: width ? { ideal: width } : undefined,
          height: height ? { ideal: height } : undefined
        },
        audio: withAudio
      })
    }
    for (const [w, h] of sized) {
      variants.push({ video: { width: { exact: w }, height: { exact: h } }, audio: withAudio })
      variants.push({ video: { width: { ideal: w }, height: { ideal: h } }, audio: withAudio })
    }
    return variants
  }, [width, height, withAudio])

  const openStream = useCallback(
    async (deviceId: CameraId | '', signal: AbortSignal) => {
      if (!videoRef.current) return
      setStatus({ state: 'opening' })

      // enumerate to validate device presence
      const devices = await navigator.mediaDevices.enumerateDevices()
      const videoInputs = devices.filter((d) => d.kind === 'videoinput')
      const hasExact = !!(deviceId && videoInputs.find((d) => d.deviceId === deviceId))

      if (deviceId && hasExact) {
        try {
          const s = await tryOpenWithVariants(buildExactVariants(deviceId), signal)
          stopActiveStream()
          currentStreamRef.current = s
          await playVideo(s)

          const vt = s.getVideoTracks()[0]
          if (vt) {
            const st = vt.getSettings?.() as MediaTrackSettings
            console.debug('[Camera] exact opened', { deviceId, settings: st })
          }

          setStatus({ state: 'ok', exactMatched: true })
          return
        } catch {
          // fall through to fallback
        }
      }

      // fallback
      if (allowFallback) {
        try {
          const s = await tryOpenWithVariants(buildFallbackVariants(), signal)
          stopActiveStream()
          currentStreamRef.current = s
          await playVideo(s)

          const vt = s.getVideoTracks()[0]
          if (vt) {
            const st = vt.getSettings?.() as MediaTrackSettings
            console.debug('[Camera] fallback opened', { settings: st })
          }

          setStatus({ state: 'ok', exactMatched: false })
          return
        } catch {}
      }

      setStatus({
        state: 'error',
        message: hasExact
          ? 'Unable to open saved camera.'
          : deviceId
            ? 'Saved camera not found.'
            : 'No camera configured.'
      })
    },
    [
      allowFallback,
      buildExactVariants,
      buildFallbackVariants,
      playVideo,
      stopActiveStream,
      tryOpenWithVariants
    ]
  )

  useEffect(() => {
    const controller = new AbortController()

    if (!savedId && !allowFallback) {
      stopActiveStream()
      setStatus({ state: 'error', message: 'No camera configured.' })
      return () => controller.abort()
    }

    openStream(savedId, controller.signal).catch((err) => {
      if (controller.signal.aborted) return
      setStatus({ state: 'error', message: err instanceof Error ? err.message : String(err) })
    })

    return () => {
      controller.abort()
      stopActiveStream()
      setStatus({ state: 'idle' })
    }
  }, [savedId, allowFallback, openStream, stopActiveStream])

  // hot-plug re-open
  useEffect(() => {
    let closed = false
    const handler = async () => {
      if (closed) return
      const controller = new AbortController()
      try {
        await openStream(savedId, controller.signal)
      } catch {}
    }
    navigator.mediaDevices.addEventListener('devicechange', handler)
    return () => {
      closed = true
      navigator.mediaDevices.removeEventListener('devicechange', handler)
    }
  }, [savedId, openStream])

  const cameraFound = status.state === 'ok'
  const exactMatched = status.state === 'ok' && status.exactMatched

  return (
    <div
      id="camera-root"
      style={{
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        position: 'relative',
        background: '#000'
      }}
    >
      <video
        ref={videoRef}
        playsInline
        muted
        style={{
          width: '100%',
          height: '100%',
          objectFit: 'cover',
          objectPosition: 'center',
          display: 'block'
        }}
      />

      {!cameraFound && (
        <Typography
          variant="subtitle1"
          style={{
            position: 'absolute',
            top: '50%',
            left: '50%',
            transform: 'translate(-50%, -50%)',
            color: '#fff',
            textAlign: 'center',
            padding: '6px 10px',
            background: 'rgba(0,0,0,0.45)',
            borderRadius: 6,
            whiteSpace: 'pre-line'
          }}
        >
          {status.state === 'opening'
            ? 'Opening camera…'
            : status.state === 'error'
              ? status.message || 'No Camera Found'
              : 'No Camera Found'}
        </Typography>
      )}

      {cameraFound && showFallbackNotice && !exactMatched && (
        <Typography
          variant="caption"
          style={{
            position: 'absolute',
            right: 8,
            bottom: 8,
            color: '#fff',
            background: 'rgba(0,0,0,0.45)',
            padding: '4px 8px',
            borderRadius: 4
          }}
        >
          Using fallback camera
        </Typography>
      )}
    </div>
  )
}
