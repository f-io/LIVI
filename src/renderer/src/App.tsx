import { useEffect, useState, useRef, useCallback } from 'react'
import { HashRouter as Router, Route, Routes, useLocation } from 'react-router-dom'
import { Home, Carplay, Camera, Info, Media, Settings } from './components/tabs'
import Nav from './components/Nav'
import { Box, Modal } from '@mui/material'
import { useCarplayStore, useStatusStore } from './store/store'
import type { KeyCommand } from '@worker/types'
import { updateCameras } from './utils/cameraDetection'

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

function broadcastMediaKey(action: string) {
  window.dispatchEvent(new CustomEvent('car-media-key', { detail: { command: action } }))
}

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

  const _FOCUSABLE_SELECTOR = [
    'button:not([disabled])',
    'a[href]',
    '[role="button"]:not([aria-disabled="true"])',
    '[role="tab"]',
    '[role="menuitem"]',
    '[role="treeitem"]',
    '[role="slider"]',
    'input:not([disabled]):not([type="hidden"])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])'
  ].join(',')

  const _isVisible = useCallback((el: HTMLElement) => {
    const r = el.getBoundingClientRect()
    return !!(el.offsetParent || el === document.body) && r.width >= 1 && r.height >= 1
  }, [])

  const getFocusableList = useCallback(
    (root: HTMLElement | null): HTMLElement[] => {
      if (!root) return []
      const all = Array.from(root.querySelectorAll<HTMLElement>(_FOCUSABLE_SELECTOR))
      return all.filter(_isVisible).filter((el) => !el.closest('[aria-hidden="true"], [inert]'))
    },
    [_FOCUSABLE_SELECTOR, _isVisible]
  )

  const getFirstFocusable = useCallback(
    (root: HTMLElement | null): HTMLElement | null => {
      const list = getFocusableList(root)
      return list[0] ?? null
    },
    [getFocusableList]
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

  const focusStepInMain = useCallback(
    (dir: 1 | -1) => {
      const list = getFocusableList(mainRef.current)
      if (!list.length) return false
      const active = document.activeElement as HTMLElement | null
      const idx = list.findIndex((el) => el === active)
      const start = idx === -1 ? (dir > 0 ? -1 : list.length) : idx
      const next = dir > 0 ? list[start + 1] : list[start - 1]
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

  const hasModalOpen = useCallback(
    () =>
      !!document.querySelector(
        '[role="dialog"][aria-modal="true"], .MuiModal-root[aria-hidden="false"]'
      ),
    []
  )
  const listboxOpen = useCallback(() => !!document.querySelector('[role="listbox"]'), [])

  useEffect(() => {
    const handleFocusChange = () => {
      if (editingField && !editingField.contains(document.activeElement)) {
        setEditingField(null)
      }
    }

    document.addEventListener('focusin', handleFocusChange)
    return () => document.removeEventListener('focusin', handleFocusChange)
  }, [editingField])

  const onKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (!settings) return

      const isCarPlayActive = location.pathname === '/' && receivingVideo

      if (isCarPlayActive) {
        if (Object.values(settings.bindings).includes(event.code)) {
          const action = Object.keys(settings.bindings).find(
            (k) => settings.bindings[k] === event.code
          )
          if (action !== undefined) {
            setKeyCommand(action!)
            setCommandCounter((p) => p + 1)
            broadcastMediaKey(action!)
            if (action === 'selectDown') {
              setTimeout(() => {
                setKeyCommand('selectUp')
                setCommandCounter((p) => p + 1)
              }, 200)
            }
            event.preventDefault()
            event.stopPropagation()
          }
        }
        return
      }

      const code = event.code
      const active = document.activeElement as HTMLElement | null
      const inNav = inContainer(navRef.current, active)
      const inMain = inContainer(mainRef.current, active)
      const nothing = !active || active === document.body

      type BindKey = 'left' | 'right' | 'up' | 'down' | 'back'
      const b = settings.bindings as Partial<Record<BindKey, string>> | undefined

      const isLeft = code === 'ArrowLeft' || b?.left === code
      const isRight = code === 'ArrowRight' || b?.right === code
      const isUp = code === 'ArrowUp' || b?.up === code
      const isDown = code === 'ArrowDown' || b?.down === code
      const isBack = b?.back === code || code === 'Escape' || code === 'Backspace'
      const isEnter = code === 'Enter' || code === 'NumpadEnter'

      if (editingField) {
        if (isBack) {
          setEditingField(null)
          editingField.blur()
          const ok = focusSelectedNav()
          if (ok) {
            event.preventDefault()
            event.stopPropagation()
          }
          return
        }
        if (isUp || isDown || isLeft || isRight) {
          return
        }
      }

      if (hasModalOpen() || listboxOpen()) return

      if (inMain && isEnter && active && !editingField) {
        const isFormField =
          active.tagName === 'INPUT' ||
          active.tagName === 'SELECT' ||
          active.tagName === 'TEXTAREA' ||
          active.getAttribute('role') === 'slider'

        if (isFormField) {
          setEditingField(active)
          if (active.tagName === 'INPUT' && (active as HTMLInputElement).type === 'number') {
            ;(active as HTMLInputElement).select()
          }
          event.preventDefault()
          event.stopPropagation()
          return
        }
      }

      if (isBack) {
        const ok = focusSelectedNav()
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      if (inNav && isEnter) {
        requestAnimationFrame(() => (document.activeElement as HTMLElement | null)?.blur())
        return
      }

      if ((isLeft || isRight || isDown) && nothing) {
        const ok = focusSelectedNav()
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      if (inNav && isDown) {
        const ok = focusFirstInMain()
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      // WICHTIG: Entferne die isInteractiveField Prüfung für Settings
      if (inMain && (isLeft || isRight)) {
        const ok = focusStepInMain(isRight ? 1 : -1)
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      if (Object.values(settings.bindings).includes(code)) {
        const action = Object.keys(settings.bindings).find((k) => settings.bindings[k] === code)
        if (action !== undefined) {
          setKeyCommand(action!)
          setCommandCounter((p) => p + 1)
          broadcastMediaKey(action!)
          if (action === 'selectDown') {
            setTimeout(() => {
              setKeyCommand('selectUp')
              setCommandCounter((p) => p + 1)
            }, 200)
          }
        }
      }
    },
    [
      settings,
      inContainer,
      hasModalOpen,
      listboxOpen,
      focusSelectedNav,
      focusFirstInMain,
      focusStepInMain,
      location.pathname,
      receivingVideo,
      editingField
    ]
  )

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
