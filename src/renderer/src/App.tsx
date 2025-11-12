import { useEffect, useState, useRef, useCallback, useContext, ElementType } from 'react'
import { HashRouter as Router, Navigate, Route, Routes, useLocation } from 'react-router-dom'
import { Carplay, Camera } from './components/tabs'
import Nav from './components/Nav'
import { Box, Modal } from '@mui/material'
import { useCarplayStore, useStatusStore } from './store/store'
import type { KeyCommand } from '@worker/types'
import { updateCameras } from './utils/cameraDetection'
import { useActiveControl, useFocus, useKeyDown } from './hooks'
import { ROUTES } from './constants'
import { AppContext } from './context'
import { RoutePath, routes, ROUTES_NEW } from './routes'

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

// TODO
// move:
// isVisible, isFormField, getFocusableList, getFirstFocusable,focusSelectedNav
// focusFirstInMain, moveFocusLinear, inContainer
// to the application context

function AppInner() {
  const appContext = useContext(AppContext)
  const [receivingVideo, setReceivingVideo] = useState(false)
  const [commandCounter, setCommandCounter] = useState(0)
  const [keyCommand, setKeyCommand] = useState('')
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

  useEffect(() => {
    if (!appContext?.navEl || !appContext?.contentEl) {
      appContext?.onSetAppContext?.({
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
        (editingField !== document.activeElement?.id ||
          editingField !== document.activeElement?.ariaLabel)
      ) {
        appContext?.onSetAppContext?.({
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
    document.addEventListener('keydown', onKeyDown, true)
    return () => document.removeEventListener('keydown', onKeyDown, true)
  }, [onKeyDown])

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
    <div style={{ height: '100%', touchAction: 'none' }} id="main" className="App">
      <div ref={navRef} id="nav-root">
        <Nav receivingVideo={receivingVideo} settings={settings} />
      </div>

      {settings && (
        <Carplay
          receivingVideo={receivingVideo}
          setReceivingVideo={setReceivingVideo}
          settings={settings}
          command={keyCommand as KeyCommand}
          commandCounter={commandCounter}
        />
      )}

      <div ref={mainRef} id="main-root">
        <Routes>
          <Route path={ROUTES_NEW.HOME} element={<Navigate to={`/${RoutePath.Home}`} />} />

          {routes.map((route, index) => {
            const Component = route.component as unknown as ElementType
            const path = route.path

            if (Component) {
              return <Route key={index} path={path} element={<Component settings={settings!} />} />
            }

            return null
          })}

          <Route path="*" element={<Navigate to={`/${RoutePath.Home}`} replace />} />
        </Routes>
      </div>

      <Modal open={reverse} onClick={() => setReverse(false)}>
        <Box sx={modalStyle}>
          <Camera settings={settings} />
        </Box>
      </Modal>
    </div>
  )
}

export default function App() {
  return (
    <Router>
      <AppInner />
    </Router>
  )
}
