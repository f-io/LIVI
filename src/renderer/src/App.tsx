import { Box } from '@mui/material'
import type { KeyCommand } from '@worker/types'
import i18n from 'i18next'
import { useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { HashRouter as Router, useLocation, useNavigate, useRoutes } from 'react-router'
import { AppLayout } from './components/layouts/AppLayout'
import { Projection } from './components/pages'
import { ROUTES } from './constants'
import { AppContext } from './context'
import { useActiveControl, useFocus, useKeyDown } from './hooks'
import { appRoutes } from './routes/appRoutes'
import { useLiviStore, useStatusStore } from './store/store'
import { broadcastMediaKey } from './utils/broadcastMediaKey'
import { updateCameras } from './utils/cameraDetection'

const START_PAGE_ROUTE: Record<string, string> = {
  home: ROUTES.HOME,
  media: ROUTES.MEDIA,
  maps: ROUTES.MAPS,
  camera: ROUTES.CAMERA,
  settings: ROUTES.SETTINGS,
  telemetry: ROUTES.TELEMETRY
}

function AppInner() {
  const appContext = useContext(AppContext)
  const [receivingVideo, setReceivingVideo] = useState(false)
  const [commandCounter, setCommandCounter] = useState(0)
  const [keyCommand, setKeyCommand] = useState('')
  const [navVideoOverlayActive, setNavVideoOverlayActive] = useState(false)
  const editingField = appContext?.keyboardNavigation?.focusedElId
  const location = useLocation()

  const navigate = useNavigate()
  const didApplyStartPageRef = useRef(false)

  const settings = useLiviStore((s) => s.settings)
  const saveSettings = useLiviStore((s) => s.saveSettings)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)

  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)

  const element = useRoutes(appRoutes)

  const lastInputModeRef = useRef<'keys' | 'pointer' | 'other'>('other')
  const prevPathRef = useRef<string>(location.pathname)
  const cameFromSettingsSubRef = useRef(false)

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

    const target = START_PAGE_ROUTE[settings.startPage ?? 'home'] ?? ROUTES.HOME

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

      if (navVideoOverlayActive && location.pathname !== ROUTES.HOME) {
        const back = settings?.bindings?.back
        const enter = settings?.bindings?.selectDown

        if (e.code === back || e.code === enter || e.key === 'Escape') {
          setNavVideoOverlayActive(false)
          e.preventDefault()
          e.stopPropagation()
          return
        }
      }

      onKeyDown(e)
    }

    document.addEventListener('keydown', handler, true)
    return () => document.removeEventListener('keydown', handler, true)
  }, [onKeyDown, navVideoOverlayActive, location.pathname, settings])

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
    window.projection.usb.listenForEvents(usbHandler)
    return () => window.projection.usb.unlistenForEvents(usbHandler)
  }, [settings, saveSettings, setCameraFound])

  return (
    <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={receivingVideo}>
      {settings && (
        <Projection
          receivingVideo={receivingVideo}
          setReceivingVideo={setReceivingVideo}
          settings={settings}
          command={keyCommand as KeyCommand}
          commandCounter={commandCounter}
          navVideoOverlayActive={navVideoOverlayActive}
          setNavVideoOverlayActive={setNavVideoOverlayActive}
        />
      )}
      <Box sx={{ width: '100%', height: '100%' }}>{element}</Box>
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
