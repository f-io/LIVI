import { useEffect, useState, useRef, useCallback, useContext, useLayoutEffect } from 'react'
import { HashRouter as Router, useLocation, useRoutes } from 'react-router'
import { Carplay, Camera } from './components/pages'
import { Box, Modal } from '@mui/material'
import { useCarplayStore, useStatusStore } from './store/store'
import type { KeyCommand } from '@worker/types'
import { updateCameras } from './utils/cameraDetection'
import { useActiveControl, useFocus, useKeyDown } from './hooks'
import { ROUTES } from './constants'
import { AppContext } from './context'
import { appRoutes } from './routes/appRoutes'
import { AppLayout } from './components/layouts/AppLayout'
import i18n from 'i18next'

const modalStyle = {
  position: 'absolute' as const,
  top: '50%',
  left: '50%',
  transform: 'translate(-50%, -50%)',
  height: '95%',
  width: '95%',
  boxShadow: 24,
  display: 'flex'
}

function AppInner() {
  const appContext = useContext(AppContext)
  const [receivingVideo, setReceivingVideo] = useState(false)
  const [commandCounter, setCommandCounter] = useState(0)
  const [keyCommand, setKeyCommand] = useState('')
  const [navVideoOverlayActive, setNavVideoOverlayActive] = useState(false)
  const editingField = appContext?.keyboardNavigation?.focusedElId
  // const [editingField, setEditingField] = useState<HTMLElement | null>(null)
  const location = useLocation()

  const reverse = useStatusStore((s) => s.reverse)
  const setReverse = useStatusStore((s) => s.setReverse)

  const settings = useCarplayStore((s) => s.settings)
  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)

  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)

  const element = useRoutes(appRoutes)

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
    if (location.pathname !== ROUTES.HOME) {
      requestAnimationFrame(() => {
        focusFirstInMain()
      })
    }
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

  useEffect(() => {
    if (!settings) return
    updateCameras(setCameraFound, saveSettings, settings)
    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as { type?: string }
      if (data.type && ['attach', 'plugged', 'detach', 'unplugged'].includes(data.type)) {
        updateCameras(setCameraFound, saveSettings, settings)
      }
    }
    window.carplay.usb.listenForEvents(usbHandler)
    return () => window.carplay.usb.unlistenForEvents(usbHandler)
  }, [settings, saveSettings, setCameraFound])

  return (
    <AppLayout navRef={navRef} mainRef={mainRef} receivingVideo={receivingVideo}>
      {settings && (
        <Carplay
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

      <Modal open={reverse} onClick={() => setReverse(false)}>
        <Box sx={modalStyle}>
          <Camera />
        </Box>
      </Modal>
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
