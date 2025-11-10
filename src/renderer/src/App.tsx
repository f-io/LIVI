import { useEffect, useState, useRef, useCallback } from 'react'
import { HashRouter as Router, Route, Routes, useLocation } from 'react-router-dom'
import { Home, Carplay, Camera, Info, Media, Settings } from './components/tabs'
import Nav from './components/Nav'
import { Box, Modal } from '@mui/material'
import { useCarplayStore, useStatusStore } from './store/store'
import type { KeyCommand } from '@worker/types'
import { updateCameras } from './utils/cameraDetection'
import { useActiveControl, useKeyDown } from './hooks'
import { FOCUSABLE_SELECTOR } from './constants'

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
  const [receivingVideo, setReceivingVideo] = useState(false)
  const [commandCounter, setCommandCounter] = useState(0)
  const [keyCommand, setKeyCommand] = useState('')
  const [editingField, setEditingField] = useState<HTMLElement | null>(null)
  const location = useLocation()

  const reverse = useStatusStore((s) => s.reverse)
  const setReverse = useStatusStore((s) => s.setReverse)

  const settings = useCarplayStore((s) => s.settings)
  const saveSettings = useCarplayStore((s) => s.saveSettings)
  const setCameraFound = useStatusStore((s) => s.setCameraFound)

  const navRef = useRef<HTMLDivElement | null>(null)
  const mainRef = useRef<HTMLDivElement | null>(null)

  const isVisible = useCallback((el: HTMLElement) => {
    const cs = window.getComputedStyle(el)
    if (cs.display === 'none' || cs.visibility === 'hidden') return false
    if (el.hasAttribute('hidden') || el.hasAttribute('disabled')) return false
    return true
  }, [])

  const isFormField = useCallback((el: HTMLElement | null) => {
    if (!el) return false
    const tag = el.tagName
    const role = el.getAttribute('role') || ''
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return true
    if (role === 'slider' || role === 'spinbutton') return true
    if (el.getAttribute('contenteditable') === 'true') return true
    return false
  }, [])

  const getFocusableList = useCallback(
    (root: HTMLElement | null): HTMLElement[] => {
      if (!root) return []
      const all = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
      return all.filter(isVisible).filter((el) => !el.closest('[aria-hidden="true"], [inert]'))
    },
    [isVisible]
  )

  const getFirstFocusable = useCallback(
    (root: HTMLElement | null): HTMLElement | null => {
      const list = getFocusableList(root)
      if (!list.length) return null
      const seed = root?.querySelector<HTMLElement>('[data-seed="first"]')
      if (seed && list.includes(seed)) return seed
      const nonForm = list.find((el) => !isFormField(el))
      return nonForm ?? list[0]
    },
    [getFocusableList, isFormField]
  )

  const focusSelectedNav = useCallback(() => {
    const target =
      (navRef.current?.querySelector('[role="tab"][aria-selected="true"]') as HTMLElement) ||
      getFirstFocusable(navRef.current)
    target?.focus({ preventScroll: true })
    return !!target
  }, [getFirstFocusable])

  const focusFirstInMain = useCallback(() => {
    const target = getFirstFocusable(mainRef.current)
    target?.focus({ preventScroll: true })
    return !!target
  }, [getFirstFocusable])

  const moveFocusLinear = useCallback(
    (delta: -1 | 1) => {
      const list = getFocusableList(mainRef.current)
      if (!list.length) return false

      const active = (document.activeElement as HTMLElement | null) ?? null
      let next: HTMLElement | null = null

      if (!active || !list.includes(active)) {
        next = delta > 0 ? list[0] : list[list.length - 1]
      } else {
        const idx = list.indexOf(active)
        const targetIdx = idx + delta
        if (targetIdx >= 0 && targetIdx < list.length) next = list[targetIdx]
      }

      if (next) {
        next.focus({ preventScroll: true })
        return true
      }
      return false
    },
    [getFocusableList]
  )

  const inContainer = useCallback(
    (container: HTMLElement | null, el: Element | null) =>
      !!(container && el && container.contains(el)),
    []
  )

  useEffect(() => {
    const handleFocusChange = () => {
      if (editingField && !editingField.contains(document.activeElement)) {
        setEditingField(null)
      }
    }
    document.addEventListener('focusin', handleFocusChange)
    return () => document.removeEventListener('focusin', handleFocusChange)
  }, [editingField])

  useEffect(() => {
    if (location.pathname !== '/') {
      requestAnimationFrame(() => {
        focusFirstInMain()
      })
    }
  }, [location.pathname, focusFirstInMain])

  const activateControl = useActiveControl()

  const onKeyDown = useKeyDown({
    settings,
    receivingVideo,
    inContainer,
    focusSelectedNav,
    focusFirstInMain,
    moveFocusLinear,
    isFormField,
    editingField,
    activateControl,
    navRef,
    mainRef,
    onSetKeyCommand: setKeyCommand,
    onSetCommandCounter: setCommandCounter,
    onSetEditingField: setEditingField
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
          <Route path="/settings" element={<Settings settings={settings!} />} />
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
