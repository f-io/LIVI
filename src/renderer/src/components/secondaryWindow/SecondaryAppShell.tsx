import { Box, Typography } from '@mui/material'
import type { Screen } from '@shared/types'
import { ROUTES } from '@shared/types'
import { isClusterOnScreen } from '@shared/utils'
import { useEffect, useRef } from 'react'
import { MemoryRouter, Route, Routes } from 'react-router'
import type { BindKey } from '../../hooks/keysControl/types'
import { reportShown, sendInput, useLiviStore, useStatusStore } from '../../store/store'
import { broadcastMediaKey } from '../../utils/broadcastMediaKey'
import { AppLayout } from '../layouts/AppLayout'
import { Camera } from '../pages/camera'
import { Cluster } from '../pages/cluster/Cluster'
import { Media } from '../pages/media'
import { Telemetry } from '../pages/telemetry'

const TRANSPORT_ACTIONS: BindKey[] = [
  'next',
  'prev',
  'playPause',
  'play',
  'pause',
  'acceptPhone',
  'rejectPhone',
  'home'
]

type Props = {
  role: Exclude<Screen, 'main'>
  emptyLabel: string
}

export const SecondaryAppShell = ({ role, emptyLabel }: Props) => {
  const settings = useLiviStore((s) => s.settings)

  const hasTelemetry =
    !!settings?.dashboards &&
    Object.values(settings.dashboards).some((slot) => slot?.[role] === true)
  const hasMedia = settings?.media?.[role] === true
  const hasCamera = settings?.camera?.[role] === true

  if (!settings) return <Box sx={{ width: '100vw', height: '100vh', bgcolor: '#000' }} />

  if (!hasTelemetry && !hasMedia && !hasCamera) {
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

  const initialPath = hasTelemetry ? ROUTES.TELEMETRY : hasMedia ? ROUTES.MEDIA : ROUTES.CAMERA

  return (
    <MemoryRouter initialEntries={[initialPath]}>
      <SecondaryShellInner role={role} />
    </MemoryRouter>
  )
}

type InnerProps = {
  role: Exclude<Screen, 'main'>
}

const SecondaryShellInner = ({ role }: InnerProps) => {
  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)
  const settings = useLiviStore((s) => s.settings)
  const clusterDashboardActive = useStatusStore((s) => s.clusterDashboardActive)
  const hasClusterDashboard = isClusterOnScreen(settings, role)

  // Core puts the cluster plane on this screen while a cluster dashboard shows
  useEffect(() => {
    reportShown(role, clusterDashboardActive ? 'cluster' : 'livi')
  }, [role, clusterDashboardActive])

  useEffect(() => {
    return window.app?.onMediaKey?.((command) => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command } }))
    })
  }, [])

  useEffect(() => {
    const bindings = settings?.bindings
    if (!bindings) return

    const codeToAction = new Map<string, BindKey>()
    for (const [action, code] of Object.entries(bindings)) {
      if (typeof code === 'string' && code) codeToAction.set(code, action as BindKey)
    }

    let pttCode: string | null = null

    const dispatch = (code: string, down: boolean, action: string) => {
      sendInput({ kind: 'key', code, down })
      broadcastMediaKey(action)
    }

    const releasePtt = () => {
      if (pttCode === null) return
      const code = pttCode
      pttCode = null
      dispatch(code, false, 'voiceAssistantRelease')
    }

    const onKeyDown = (e: KeyboardEvent) => {
      const action = codeToAction.get(e.code)
      if (!action) return

      if (action === 'voiceAssistant') {
        if (e.repeat) {
          e.preventDefault()
          return
        }
        pttCode = e.code
        dispatch(e.code, true, 'voiceAssistant')
        e.preventDefault()
        return
      }

      if (TRANSPORT_ACTIONS.includes(action)) {
        dispatch(e.code, true, action)
        e.preventDefault()
      }
    }

    const onKeyUp = (e: KeyboardEvent) => {
      const action = codeToAction.get(e.code)
      if (action === 'voiceAssistant') releasePtt()
    }

    const onBlur = () => releasePtt()
    const onVisibility = () => {
      if (document.visibilityState !== 'visible') releasePtt()
    }

    document.addEventListener('keydown', onKeyDown, true)
    document.addEventListener('keyup', onKeyUp, true)
    window.addEventListener('blur', onBlur)
    document.addEventListener('visibilitychange', onVisibility)

    return () => {
      document.removeEventListener('keydown', onKeyDown, true)
      document.removeEventListener('keyup', onKeyUp, true)
      window.removeEventListener('blur', onBlur)
      document.removeEventListener('visibilitychange', onVisibility)
      releasePtt()
    }
  }, [settings])

  return (
    <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={false}>
      {hasClusterDashboard && settings && (
        <Cluster
          visible={clusterDashboardActive}
          showLoadingPlaceholder={!clusterDashboardActive}
        />
      )}
      <Routes>
        <Route path={ROUTES.TELEMETRY} element={<Telemetry windowRole={role} />} />
        <Route path={ROUTES.MEDIA} element={<Media />} />
        <Route path={ROUTES.CAMERA} element={<Camera />} />
        <Route path="*" element={null} />
      </Routes>
    </AppLayout>
  )
}
