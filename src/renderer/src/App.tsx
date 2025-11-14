import { useEffect, useState, useRef, useCallback, useContext } from 'react'
import { HashRouter as Router, Route, Routes, useLocation } from 'react-router-dom'
import { Home, Carplay, Camera, Info, Media, Settings } from './components/tabs'
import Nav from './components/Nav'
import { Box, Modal } from '@mui/material'
import { useCarplayStore, useStatusStore } from './store/store'
import type { KeyCommand } from '@worker/types'
import { updateCameras } from './utils/cameraDetection'
import { useActiveControl, useFocus, useKeyDown } from './hooks'
import { ROUTES } from './constants'
import { AppContext } from './context'

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
    // settings,
    receivingVideo,
    inContainer,
    focusSelectedNav,
    focusFirstInMain,
    moveFocusLinear,
    isFormField,
    // editingField,
    activateControl,
    onSetKeyCommand: setKeyCommand,
    onSetCommandCounter: setCommandCounter
    // onSetEditingField: setEditingField
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
          <Route path="/" element={<Home />} />
          <Route path="/media" element={<Media />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="/info" element={<Info />} />
          <Route path="/camera" element={<Camera settings={settings!} />} />
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
