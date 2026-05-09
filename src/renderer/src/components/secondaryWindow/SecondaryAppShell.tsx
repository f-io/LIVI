import MapOutlinedIcon from '@mui/icons-material/MapOutlined'
import PlayCircleOutlinedIcon from '@mui/icons-material/PlayCircleOutlined'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'
import { Box, Typography } from '@mui/material'
import type { WindowId } from '@shared/types'
import { useRef, useState } from 'react'
import { useAutoHideNav } from '../../hooks/useAutoHideNav'
import { useLiviStore } from '../../store/store'
import { NavColumn } from '../navigation/NavColumn'
import type { NavRailItem } from '../navigation/NavRail'
import { Cluster } from '../pages/cluster/Cluster'
import { Media } from '../pages/media'
import { Telemetry } from '../pages/telemetry'

type Props = {
  role: Exclude<WindowId, 'main'>
  emptyLabel: string
}

const AUTO_HIDE_KEYS = new Set(['cluster', 'telemetry'])

export const SecondaryAppShell = ({ role, emptyLabel }: Props) => {
  const settings = useLiviStore((s) => s.settings)
  const navColRef = useRef<HTMLDivElement | null>(null)

  const hasTelemetry =
    !!settings?.dashboards &&
    Object.values(settings.dashboards).some((slot) => slot?.[role] === true)
  const hasMedia = settings?.media?.[role] === true
  const hasCluster = settings?.clusterEnabled === true && settings?.cluster?.[role] === true

  const items: NavRailItem[] = []
  if (hasCluster) {
    items.push({
      key: 'cluster',
      label: 'Cluster',
      icon: <MapOutlinedIcon sx={{ fontSize: 30 }} />
    })
  }
  if (hasTelemetry) {
    items.push({
      key: 'telemetry',
      label: 'Telemetry',
      icon: <SpeedOutlinedIcon sx={{ fontSize: 30 }} />
    })
  }
  if (hasMedia) {
    items.push({
      key: 'media',
      label: 'Media',
      icon: <PlayCircleOutlinedIcon sx={{ fontSize: 30 }} />
    })
  }
  // Future: A/C controls, etc — push more items here as new content types
  // become available on secondary windows.

  const [view, setView] = useState<string>(items[0]?.key ?? 'telemetry')
  const effectiveKey = items.find((t) => t.key === view)?.key ?? items[0]?.key ?? ''

  // Mirror AppLayout's auto-hide: nav fades out after inactivity on cluster
  // / telemetry views, fades back in on activity. Disabled when there's only
  // one tab (the rail isn't rendered then anyway).
  const autoHideEnabled = items.length > 1 && AUTO_HIDE_KEYS.has(effectiveKey)
  const { hidden: navHidden } = useAutoHideNav(autoHideEnabled, navColRef.current)

  if (!settings) return <Box sx={{ width: '100vw', height: '100vh', bgcolor: '#000' }} />

  if (items.length === 0) {
    return (
      <Box
        sx={{
          width: '100vw',
          height: '100vh',
          bgcolor: '#000',
          color: 'rgba(255,255,255,0.45)',
          display: 'grid',
          placeItems: 'center'
        }}
      >
        <Typography variant="h6">{emptyLabel}</Typography>
      </Box>
    )
  }

  return (
    <Box
      sx={{
        width: '100vw',
        height: '100vh',
        overflow: 'hidden',
        bgcolor: '#000',
        display: 'flex',
        flexDirection: 'row'
      }}
    >
      {items.length > 1 && (
        <NavColumn
          ref={navColRef}
          items={items}
          activeKey={effectiveKey}
          onSelect={setView}
          hidden={navHidden}
          ariaLabel="Secondary Window Navigation"
        />
      )}

      <Box
        id="content-root"
        data-nav-hidden={navHidden ? '1' : '0'}
        sx={{
          flex: 1,
          minWidth: 0,
          height: '100%',
          position: 'relative',
          overflow: 'hidden'
        }}
      >
        {hasCluster && <Cluster visible={effectiveKey === 'cluster'} />}
        {effectiveKey === 'telemetry' && <Telemetry windowRole={role} />}
        {effectiveKey === 'media' && <Media forceHydrate />}
      </Box>
    </Box>
  )
}
