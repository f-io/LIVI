import { Box } from '@mui/material'
import { ROUTES } from '@shared/types'
import type { KeyCommand } from '@worker/types'
import i18n from 'i18next'
import { useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react'
import {
  matchRoutes,
  HashRouter as Router,
  useLocation,
  useNavigate,
  useRoutes
} from 'react-router'
import { SessionSwitchOverlay } from './components/common/SessionSwitchOverlay'
import { AppLayout } from './components/layouts/AppLayout'
import { Cluster, Projection } from './components/pages'
import { AppContext } from './context'
import { useActiveControl, useFocus, useKeyDown } from './hooks'
import { appRoutes } from './routes/appRoutes'
import { useLiviStore, useStatusStore } from './store/store'
import { broadcastMediaKey } from './utils/broadcastMediaKey'
import { updateCameras } from './utils/cameraDetection'
import { getWindowRole } from './utils/windowRole'

function AppInner() {
  const appContext = useContext(AppContext)
  const [receivingVideo, setReceivingVideo] = useState(false)
  const [commandCounter, setCommandCounter] = useState(0)
  const [keyCommand, setKeyCommand] = useState('')
  const editingField = appContext?.keyboardNavigation?.focusedElId
  const location = useLocation()

  const navigate = useNavigate()
  const didApplyStartPageRef = useRef(false)

  const settings = useLiviStore((s) => s.settings)
  const saveSettings = useLiviStore((s) => s.saveSettings)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)
  const clusterDashActive = useStatusStore((s) => s.clusterDashActive)

  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)

  const element = useRoutes(appRoutes)

  const lastInputModeRef = useRef<'keys' | 'pointer' | 'other'>('other')
  const prevPathRef = useRef<string>(location.pathname)
  const cameFromSettingsSubRef = useRef(false)

  useEffect(() => {
    window.app?.reportPath?.(location.pathname)
  }, [location.pathname])

  // Subscribe to main-process media key broadcasts
  useEffect(() => {
    return window.app?.onMediaKey?.((command) => {
      window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command } }))
    })
  }, [])

  // Track input mode globally (for CSS that must behave differently on touch vs mouse)
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

    // default
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
    activateControl,
    onSetKeyCommand: setKeyCommand,
    onSetCommandCounter: setCommandCounter
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

  // PTT release: dispatch on keyup, blur, or visibility loss.
  useEffect(() => {
    if (!settings) return
    const binding = settings.bindings?.voiceAssistant
    if (!binding) return

    let pressed = false

    const dispatchRelease = () => {
      if (!pressed) return
      pressed = false
      setKeyCommand('voiceAssistantRelease' as KeyCommand)
      setCommandCounter((p) => p + 1)
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
    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as { type?: string }
      if (data.type && ['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        updateCameras(setCameraFound, saveSettings, settings)
      }
    }
    const unsubscribe = window.projection.usb.listenForEvents(usbHandler)
    return unsubscribe
  }, [settings, saveSettings, setCameraFound])

  const reverse = useStatusStore((s) => s.reverse)
  const cameraFound = useStatusStore((s) => s.cameraFound)
  const reverseAutoSwitchActiveRef = useRef(false)
  const reverseBackPathRef = useRef<string | null>(null)
  const cameraOnRole = (() => {
    const role = getWindowRole()
    return role === 'main' ? (settings?.camera?.main ?? true) : (settings?.camera?.[role] ?? false)
  })()
  useEffect(() => {
    if (!settings?.autoSwitchOnReverse) return
    if (!cameraOnRole) return
    const cameraReady = cameraFound && Boolean(settings.cameraId)

    if (reverse && cameraReady) {
      if (location.pathname !== ROUTES.CAMERA) {
        reverseBackPathRef.current = location.pathname
        reverseAutoSwitchActiveRef.current = true
        navigate(ROUTES.CAMERA)
      }
      return
    }

    // reverse off: restore the previous route
    if (reverseAutoSwitchActiveRef.current && location.pathname === ROUTES.CAMERA) {
      const back = reverseBackPathRef.current as string
      reverseAutoSwitchActiveRef.current = false
      reverseBackPathRef.current = null
      navigate(back)
    }
  }, [
    reverse,
    cameraFound,
    cameraOnRole,
    settings?.autoSwitchOnReverse,
    settings?.cameraId,
    location.pathname,
    navigate
  ])

  // External navigation request
  const requestedPath = useStatusStore((s) => s.requestedPath)
  const clearRequestedPath = useStatusStore((s) => s.clearRequestedPath)
  useEffect(() => {
    if (!requestedPath) return
    clearRequestedPath()
    const target = requestedPath.replace(/\/{2,}/g, '/').replace(/(.)\/$/, '$1')
    const matches = matchRoutes(appRoutes, target)
    const leaf = matches?.[matches.length - 1]
    if (!leaf || (leaf.route.path?.endsWith('*') && leaf.params['*'])) return
    if (location.pathname !== target) navigate(target)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestedPath])

  return (
    <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={receivingVideo}>
      {settings && (
        <Projection
          receivingVideo={receivingVideo}
          setReceivingVideo={setReceivingVideo}
          settings={settings}
          command={keyCommand as KeyCommand}
          commandCounter={commandCounter}
        />
      )}
      {/* Single cluster overlay, owns the plane reveal. A telemetry dash hosting the
          cluster drives it. */}
      {settings && (
        <Cluster visible={clusterDashActive} showLoadingPlaceholder={!clusterDashActive} />
      )}
      <Box sx={{ width: '100%', height: '100%' }}>{element}</Box>
      <SessionSwitchOverlay />
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
