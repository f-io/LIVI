import { Box, Typography } from '@mui/material'
import type { WindowId } from '@shared/types'
import { useRef } from 'react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { ROUTES } from '../../constants'
import { useLiviStore } from '../../store/store'
import { AppLayout } from '../layouts/AppLayout'
import { Cluster } from '../pages/cluster/Cluster'
import { Media } from '../pages/media'
import { Telemetry } from '../pages/telemetry'

type Props = {
  role: Exclude<WindowId, 'main'>
  emptyLabel: string
}

export const SecondaryAppShell = ({ role, emptyLabel }: Props) => {
  const settings = useLiviStore((s) => s.settings)

  const hasTelemetry =
    !!settings?.dashboards &&
    Object.values(settings.dashboards).some((slot) => slot?.[role] === true)
  const hasMedia = settings?.media?.[role] === true
  const hasCluster = settings?.clusterEnabled === true && settings?.cluster?.[role] === true

  if (!settings) return <Box sx={{ width: '100vw', height: '100vh', bgcolor: '#000' }} />

  if (!hasCluster && !hasTelemetry && !hasMedia) {
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

  const initialPath = hasCluster ? ROUTES.CLUSTER : hasTelemetry ? ROUTES.TELEMETRY : ROUTES.MEDIA

  return (
    <MemoryRouter initialEntries={[initialPath]}>
      <SecondaryShellInner role={role} hasCluster={hasCluster} />
    </MemoryRouter>
  )
}

type InnerProps = {
  role: Exclude<WindowId, 'main'>
  hasCluster: boolean
}

const SecondaryShellInner = ({ role, hasCluster }: InnerProps) => {
  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)
  const { pathname } = useLocation()
  const settings = useLiviStore((s) => s.settings)

  return (
    <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
      {hasCluster && settings && <Cluster visible={pathname === ROUTES.CLUSTER} />}
      <Routes>
        <Route path={ROUTES.TELEMETRY} element={<Telemetry windowRole={role} />} />
        <Route path={ROUTES.MEDIA} element={<Media forceHydrate />} />
        <Route path={ROUTES.CLUSTER} element={null} />
        <Route path="*" element={null} />
      </Routes>
    </AppLayout>
  )
}
