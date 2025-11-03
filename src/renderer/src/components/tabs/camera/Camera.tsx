import React, { useEffect, useRef, useState } from 'react'
import { Typography } from '@mui/material'

interface CameraProps {
  settings: { camera: string } | null
}

export const Camera: React.FC<CameraProps> = ({ settings }) => {
  const videoRef = useRef<HTMLVideoElement>(null)
  const [cameraFound, setCameraFound] = useState(false)

  useEffect(() => {
    let activeStream: MediaStream | null = null

    if (!settings?.camera) {
      setCameraFound(false)
      return
    }

    navigator.mediaDevices
      .getUserMedia({ video: { width: 800, deviceId: settings.camera } })
      .then((stream) => {
        activeStream = stream
        setCameraFound(true)
        if (videoRef.current) {
          videoRef.current.srcObject = stream
          videoRef.current.play()
        }
      })
      .catch((err) => {
        console.error('error:', err)
        setCameraFound(false)
      })

    // Cleanup
    return () => {
      if (activeStream) {
        activeStream.getTracks().forEach((track) => track.stop())
      }
      if (videoRef.current) {
        videoRef.current.srcObject = null
      }
      setCameraFound(false)
    }
  }, [settings?.camera])

  return (
    <div
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
