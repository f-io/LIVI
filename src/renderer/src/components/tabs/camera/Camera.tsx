import React, { useEffect, useRef, useState } from 'react'
import { Typography } from '@mui/material'

interface CameraProps {
  settings: { camera: string } | null
}

export const Camera: React.FC<CameraProps> = ({ settings }) => {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [cameraFound, setCameraFound] = useState(false)

  useEffect(() => {
    const videoEl = videoRef.current
    let activeStream: MediaStream | null = null
    let cancelled = false

    if (!settings?.camera || !videoEl) {
      setCameraFound(false)
      return
    }

    navigator.mediaDevices
      .getUserMedia({ video: { width: 800, deviceId: settings.camera } })
      .then((stream) => {
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop())
          return
        }
        activeStream = stream
        setCameraFound(true)
        videoEl.srcObject = stream
        const p = videoEl.play()
        // Avoid unhandled promise rejection on autoplay restrictions

        if (p && typeof (p as Promise<void>).catch === 'function')
          (p as Promise<void>).catch(() => {})
      })
      .catch((err) => {
        if (!cancelled) {
          console.error('error:', err)
          setCameraFound(false)
        }
      })

    return () => {
      cancelled = true
      if (activeStream) {
        activeStream.getTracks().forEach((track) => track.stop())
      }
      if (videoEl) {
        try {
          videoEl.pause()
        } catch {}
        ;(videoEl as HTMLVideoElement).srcObject = null
      }
      setCameraFound(false)
    }
  }, [settings?.camera])

  return (
    <div
      id="camera-root"
      style={{
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        position: 'relative'
      }}
    >
      <video
        ref={videoRef}
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
            color: '#fff'
          }}
        >
          No Camera Found
        </Typography>
      )}
    </div>
  )
}
