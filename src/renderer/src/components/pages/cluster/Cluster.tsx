import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import { Box, useTheme } from '@mui/material'
import React, { useEffect, useRef, useState } from 'react'
import { useLiviStore, useStatusStore } from '../../../store/store'
import { ViewAreaMask } from '../projection/ViewAreaMask'

type ClusterProps = {
  visible?: boolean
  showLoadingPlaceholder?: boolean
}

export const Cluster: React.FC<ClusterProps> = ({ visible, showLoadingPlaceholder = true }) => {
  const theme = useTheme()
  const showCluster = visible === true

  const settings = useLiviStore((s) => s.settings)
  const clusterDashActive = useStatusStore((s) => s.clusterDashActive)

  const [clusterStreamActive, setClusterStreamActive] = useState(false)

  const rootRef = useRef<HTMLDivElement>(null)

  // Request the cluster (stream + plane) ONLY while the cluster view is actually shown.
  const showClusterRef = useRef(showCluster)
  useEffect(() => {
    showClusterRef.current = showCluster
  }, [showCluster])

  useEffect(() => {
    void window.projection.ipc.requestCluster(showCluster).catch(() => {})
  }, [showCluster])

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as { type?: string }
      if (msg.type !== 'plugged') return
      void window.projection.ipc.requestCluster(showClusterRef.current).catch(() => {})
    }
    const unsubscribe = window.projection.ipc.onEvent(handler)
    return unsubscribe
  }, [])

  // Cluster frames negotiated -> the compositor renders the cluster plane
  useEffect(() => {
    const ipc = window.projection.ipc as {
      onClusterResolution?: (cb: (payload: unknown) => void) => (() => void) | void
    }
    if (typeof ipc.onClusterResolution !== 'function') return
    const off = ipc.onClusterResolution((payload: unknown) => {
      const d = payload as { width?: number; height?: number } | undefined
      const w = typeof d?.width === 'number' ? d.width : 0
      const h = typeof d?.height === 'number' ? d.height : 0
      if (w > 0 && h > 0) setClusterStreamActive(true)
    })
    return typeof off === 'function' ? off : undefined
  }, [])

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = (args[0] ?? {}) as { type?: string }
      if (msg.type !== 'unplugged' && msg.type !== 'failure') return
      setClusterStreamActive(false)
      void window.projection.ipc.requestCluster(false).catch(() => {})
    }
    const unsubscribe = window.projection.ipc.onEvent(handler)
    return unsubscribe
  }, [])

  useEffect(() => {
    if (!clusterStreamActive || !clusterDashActive) return
    const ipc = window.projection.ipc as { clusterRepaintNudge?: () => Promise<unknown> }
    const id = setTimeout(() => {
      void ipc.clusterRepaintNudge?.().catch(() => {})
    }, 120)
    return () => clearTimeout(id)
  }, [clusterStreamActive, clusterDashActive])

  // The cluster is a video plane below the UI (compositor plane on Linux, NSView on mac).
  useEffect(() => {
    document.documentElement.classList.toggle('show-cluster', showCluster)
    return () => {
      if (showCluster) document.documentElement.classList.remove('show-cluster')
    }
  }, [showCluster])

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
        backgroundColor: 'transparent',
        visibility: showCluster ? 'visible' : 'hidden',
        opacity: showCluster ? 1 : 0,
        pointerEvents: 'none',
        transition: 'opacity 220ms ease',
        zIndex: showCluster ? 0 : -1
      }}
    >
      {showLoadingPlaceholder && !clusterStreamActive && showCluster && (
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

      <ViewAreaMask
        visible={showCluster && clusterStreamActive}
        displayWidth={settings?.clusterWidth ?? 0}
        displayHeight={settings?.clusterHeight ?? 0}
        insets={{
          top: settings?.clusterViewAreaTop ?? 0,
          bottom: settings?.clusterViewAreaBottom ?? 0,
          left: settings?.clusterViewAreaLeft ?? 0,
          right: settings?.clusterViewAreaRight ?? 0
        }}
      />
    </Box>
  )
}
