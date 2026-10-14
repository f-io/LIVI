import { Box } from '@mui/material'
import { ROUTES } from '@shared/types'
import i18n from 'i18next'
import { useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
  matchRoutes,
  HashRouter as Router,
  useLocation,
  useNavigate,
  useRoutes
} from 'react-router'
import { PhoneOverlay } from './components/common/PhoneOverlay'
import { VolumeOverlay } from './components/common/VolumeOverlay'
import { AppLayout } from './components/layouts/AppLayout'
import { Cluster, Projection } from './components/pages'
import { AppContext } from './context'
import { useActiveControl, useFocus, useFollowFront, useKeyDown } from './hooks'
import { appRoutes } from './routes/appRoutes'
import {
  onOpenPage,
  reportCamera,
  reportPath,
  sendInput,
  useLiviStore,
  useStatusStore
} from './store/store'
import { broadcastMediaKey } from './utils/broadcastMediaKey'
import { updateCameras } from './utils/cameraDetection'

function AppInner() {
  const appContext = useContext(AppContext)
  const [receivingVideo, setReceivingVideo] = useState(false)
  const editingField = appContext?.keyboardNavigation?.focusedElId
  const location = useLocation()

  const navigate = useNavigate()
  const didApplyStartPageRef = useRef(false)

  const settings = useLiviStore((s) => s.settings)
  const saveSettings = useLiviStore((s) => s.saveSettings)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)
  const clusterDashboardActive = useStatusStore((s) => s.clusterDashboardActive)

  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)

  const element = useRoutes(appRoutes)

  const lastInputModeRef = useRef<'keys' | 'pointer' | 'other'>('other')
  const prevPathRef = useRef<string>(location.pathname)
  const cameFromSettingsSubRef = useRef(false)

  useEffect(() => {
    reportPath(location.pathname)
  }, [location.pathname])

  useEffect(() => {
    return window.app?.onMediaKey?.((command) => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command } }))
    })
  }, [])

  // The input mode lets CSS tell touch, mouse and keys apart.
  useEffect(() => {
    const setMode = (mode: 'mouse' | 'touch' | 'keys') => {
      document.documentElement.dataset.input = mode
    }

    const onPointerDown = (e: PointerEvent) => {
      lastInputModeRef.current = 'pointer'
      setMode(e.pointerType === 'mouse' ? 'mouse' : 'touch')
    }

    const onKeyDown = () => {
      lastInputModeRef.current = 'keys'
      setMode('keys')
    }

    document.addEventListener('pointerdown', onPointerDown, true)
    document.addEventListener('keydown', onKeyDown, true)

    setMode('keys')

    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true)
      document.removeEventListener('keydown', onKeyDown, true)
    }
  }, [])

  useEffect(() => {
    const prev = prevPathRef.current
    const next = location.pathname
    prevPathRef.current = next

    const prevIsSettingsSub = prev.startsWith('/settings/') && prev !== '/settings'
    const nextIsSettingsRoot = next === '/settings'

    cameFromSettingsSubRef.current = prevIsSettingsSub && nextIsSettingsRoot
  }, [location.pathname])

  useEffect(() => {
    if (!settings) return
    if (didApplyStartPageRef.current) return

    if (location.pathname !== ROUTES.HOME) {
      didApplyStartPageRef.current = true
      return
    }

    const target = settings.startPage ?? ROUTES.HOME

    didApplyStartPageRef.current = true

    if (target !== ROUTES.HOME) {
      navigate(target, { replace: true })
    }
  }, [settings, location.pathname, navigate])

  useLayoutEffect(() => {
    i18n.changeLanguage(settings?.language || 'en')
  }, [settings?.language])

  useEffect(() => {
    if (!appContext?.navEl || !appContext?.contentEl) {
      appContext?.onSetAppContext?.({
        ...appContext,
        navEl: navRef,
        contentEl: mainRef
      })
    }
  }, [appContext])

  const { isFormField, focusSelectedNav, focusFirstInMain, moveFocusLinear } = useFocus()

  const inContainer = useCallback(
    (container?: HTMLElement | null, el?: Element | null) =>
      !!(container && el && container.contains(el)),
    []
  )

  useEffect(() => {
    const handleFocusChange = () => {
      if (
        editingField &&
        !appContext.isTouchDevice &&
        (editingField !== document.activeElement?.id ||
          editingField !== document.activeElement?.ariaLabel)
      ) {
        appContext?.onSetAppContext?.({
          ...appContext,
          keyboardNavigation: {
            focusedElId: null
          }
        })
      }
    }
    document.addEventListener('focusin', handleFocusChange)
    return () => document.removeEventListener('focusin', handleFocusChange)
  }, [appContext, editingField])

  useEffect(() => {
    if (location.pathname === ROUTES.HOME) return
    if (lastInputModeRef.current !== 'keys') return

    requestAnimationFrame(() => {
      focusFirstInMain()
    })
  }, [location.pathname, focusFirstInMain])

  const activateControl = useActiveControl()

  const onKeyDown = useKeyDown({
    receivingVideo,
    inContainer,
    focusSelectedNav,
    focusFirstInMain,
    moveFocusLinear,
    isFormField,
    activateControl
  })

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      lastInputModeRef.current = 'keys'
      document.documentElement.dataset.input = 'keys'

      onKeyDown(e)
    }

    document.addEventListener('keydown', handler, true)
    return () => document.removeEventListener('keydown', handler, true)
  }, [onKeyDown])

  // A lost keyup must never leave PTT held, so blur and hiding release it too.
  useEffect(() => {
    if (!settings) return
    const binding = settings.bindings?.voiceAssistant
    if (!binding) return

    let pressed = false

    const dispatchRelease = () => {
      if (!pressed) return
      pressed = false
      sendInput({ kind: 'key', code: binding, down: false })
      broadcastMediaKey('voiceAssistantRelease')
    }

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.code === binding && !e.repeat) pressed = true
    }
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === binding) dispatchRelease()
    }
    const onBlur = () => dispatchRelease()
    const onVisibility = () => {
      if (document.visibilityState !== 'visible') dispatchRelease()
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
      dispatchRelease()
    }
  }, [settings])

  useEffect(() => {
    if (!settings) return
    updateCameras(setCameraFound, saveSettings, settings)
  }, [settings, saveSettings, setCameraFound])

  const cameraFound = useStatusStore((s) => s.cameraFound)
  useEffect(() => {
    reportCamera(cameraFound)
  }, [cameraFound])

  useFollowFront()

  const pathnameRef = useRef(location.pathname)
  pathnameRef.current = location.pathname
  useEffect(
    () =>
      onOpenPage((path) => {
        const target = path.replace(/\/{2,}/g, '/').replace(/(.)\/$/, '$1')
        const matches = matchRoutes(appRoutes, target)
        const leaf = matches?.[matches.length - 1]
        if (!leaf || (leaf.route.path?.endsWith('*') && leaf.params['*'])) return
        if (pathnameRef.current !== target) navigate(target)
      }),
    [navigate]
  )

  return (
    <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={receivingVideo}>
      {settings && (
        <Projection
          receivingVideo={receivingVideo}
          setReceivingVideo={setReceivingVideo}
          settings={settings}
        />
      )}
      {/* The only cluster overlay, it owns the plane reveal. */}
      {settings && (
        <Cluster
          visible={clusterDashboardActive}
          showLoadingPlaceholder={!clusterDashboardActive}
        />
      )}
      <Box sx={{ width: '100%', height: '100%' }}>{element}</Box>
      <PhoneOverlay />
      <VolumeOverlay />
    </AppLayout>
  )
}

export default function App() {
  return (
    <Router>
      <AppInner />
    </Router>
  )
}
