import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import { Box, useTheme } from '@mui/material'
import React, { useEffect, useRef } from 'react'
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
  const clusterDashboardActive = useStatusStore((s) => s.clusterDashboardActive)

  // Core streams the cluster onto this screen's plane while a phone is there.
  const clusterStreamActive = useLiviStore((s) => s.sessions.active !== null)

  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!clusterStreamActive || !clusterDashboardActive) return
    const ipc = window.projection.ipc as { clusterRepaintNudge?: () => Promise<unknown> }
    const id = setTimeout(() => {
      void ipc.clusterRepaintNudge?.().catch(() => {})
    }, 120)
    return () => clearTimeout(id)
  }, [clusterStreamActive, clusterDashboardActive])

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
