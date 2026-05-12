import { Box, Typography } from '@mui/material'
import type { WindowId } from '@shared/types'
import { useEffect, useRef } from 'react'
import { MemoryRouter, Route, Routes, useLocation } from 'react-router'
import { ROUTES } from '../../constants'
import type { BindKey } from '../../hooks/keysControl/types'
import { useLiviStore } from '../../store/store'
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
  role: Exclude<WindowId, 'main'>
  emptyLabel: string
}

export const SecondaryAppShell = ({ role, emptyLabel }: Props) => {
  const settings = useLiviStore((s) => s.settings)

  const hasTelemetry =
    !!settings?.dashboards &&
    Object.values(settings.dashboards).some((slot) => slot?.[role] === true)
  const hasMedia = settings?.media?.[role] === true
  const hasCluster = settings?.cluster?.[role] === true
  const hasCamera = settings?.camera?.[role] === true

  if (!settings) return <Box sx={{ width: '100vw', height: '100vh', bgcolor: '#000' }} />

  if (!hasCluster && !hasTelemetry && !hasMedia && !hasCamera) {
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

  const initialPath = hasCluster
    ? ROUTES.CLUSTER
    : hasTelemetry
      ? ROUTES.TELEMETRY
      : hasMedia
        ? ROUTES.MEDIA
        : ROUTES.CAMERA

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

  // Receive media-key broadcasts from main so this window's Media UI also flashes.
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

    let pttPressed = false

    const dispatch = (cmd: string) => {
      try {
        window.projection.ipc.sendCommand(cmd)
      } catch (e) {
        console.warn('[secondary keys] sendCommand failed', e)
      }
      broadcastMediaKey(cmd)
    }

    const releasePtt = () => {
      if (!pttPressed) return
      pttPressed = false
      dispatch('voiceAssistantRelease')
    }

    const onKeyDown = (e: KeyboardEvent) => {
      const action = codeToAction.get(e.code)
      if (!action) return

      if (action === 'voiceAssistant') {
        if (e.repeat) {
          e.preventDefault()
          return
        }
        pttPressed = true
        dispatch('voiceAssistant')
        e.preventDefault()
        return
      }

      if (TRANSPORT_ACTIONS.includes(action)) {
        dispatch(action)
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
      {hasCluster && settings && <Cluster visible={pathname === ROUTES.CLUSTER} />}
      <Routes>
        <Route path={ROUTES.TELEMETRY} element={<Telemetry windowRole={role} />} />
        <Route path={ROUTES.MEDIA} element={<Media forceHydrate />} />
        <Route path={ROUTES.CAMERA} element={<Camera />} />
        <Route path={ROUTES.CLUSTER} element={null} />
        <Route path="*" element={null} />
      </Routes>
    </AppLayout>
  )
}
