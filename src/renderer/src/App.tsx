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

type BindKey =
  | 'left'
  | 'right'
  | 'up'
  | 'down'
  | 'back'
  | 'selectDown'
  | 'next'
  | 'prev'
  | 'play'
  | 'pause'
  | 'seekFwd'
  | 'seekBack'

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

  const FOCUSABLE_SELECTOR = [
    'button:not([disabled])',
    'a[href]',
    '[role="button"]:not([aria-disabled="true"])',
    '[role="tab"]',
    '[role="menuitem"]',
    '[role="treeitem"]',
    '[role="slider"]',
    '[role="spinbutton"]',
    '[role="switch"]',
    'input:not([disabled]):not([type="hidden"])',
    'input[type="checkbox"]:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[tabindex]:not([tabindex="-1"])'
  ].join(',')

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
    [FOCUSABLE_SELECTOR, isVisible]
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

  const activateControl = useCallback((el: HTMLElement | null) => {
    if (!el) return false

    const isSwitchLike =
      el.getAttribute('role') === 'switch' ||
      (el.tagName === 'INPUT' && (el as HTMLInputElement).type === 'checkbox')

    const isDropdownButton =
      el.getAttribute('role') === 'combobox' && el.getAttribute('aria-haspopup') === 'listbox'

    const clickable =
      el.closest<HTMLElement>(
        '[role="button"][aria-haspopup="listbox"],[role="switch"],button,[role="button"],a,label,[for]'
      ) ||
      el.querySelector<HTMLElement>(
        '[role="button"][aria-haspopup="listbox"],[role="switch"],button,[role="button"],a,label,[for]'
      ) ||
      el.querySelector<HTMLElement>(
        '[role="combobox"][aria-haspopup="listbox"],[role="switch"],button,[role="button"],a,label,[for]'
      ) ||
      el

    if (isSwitchLike || (typeof clickable.click === 'function' && !isDropdownButton)) {
      clickable.click()
      return true
    }

    if (isDropdownButton) {
      const evt = new MouseEvent('mousedown', { bubbles: true, cancelable: true })
      clickable.dispatchEvent(evt)
      return true
    }

    const evt = new MouseEvent('click', { bubbles: true, cancelable: true })
    return clickable.dispatchEvent(evt)
  }, [])

  const onKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (!settings) return

      const code = event.code
      const active = document.activeElement as HTMLElement | null
      const isCarPlayActive = location.pathname === '/' && receivingVideo

      const b = settings.bindings as Partial<Record<BindKey, string>> | undefined

      const isLeft = code === 'ArrowLeft' || b?.left === code
      const isRight = code === 'ArrowRight' || b?.right === code
      const isUp = code === 'ArrowUp' || b?.up === code
      const isDown = code === 'ArrowDown' || b?.down === code
      const isBackKey = b?.back === code || code === 'Escape'
      const isEnter = code === 'Enter' || code === 'NumpadEnter'
      const isSelectDown = !!b?.selectDown && code === b?.selectDown

      let mappedAction: BindKey | undefined
      for (const [k, v] of Object.entries(b ?? {})) {
        if (v === code) {
          mappedAction = k as BindKey
          break
        }
      }

      if (isCarPlayActive && mappedAction) {
        setKeyCommand(mappedAction as KeyCommand)
        setCommandCounter((p) => p + 1)
        broadcastMediaKey(mappedAction)
        if (mappedAction === 'selectDown') {
          setTimeout(() => {
            setKeyCommand('selectUp' as KeyCommand)
            setCommandCounter((p) => p + 1)
            broadcastMediaKey('selectUp')
          }, 200)
        }
        event.preventDefault()
        event.stopPropagation()
        return
      }

      const inNav = inContainer(navRef.current, active)
      const inMain = inContainer(mainRef.current, active)
      const nothing = !active || active === document.body
      const formFocused = isFormField(active)

      if (inNav && isEnter) {
        requestAnimationFrame(() => {
          focusFirstInMain()
        })
        return
      }

      if (location.pathname !== '/' && nothing && (isLeft || isRight || isUp || isDown)) {
        const okMain = focusFirstInMain()
        if (okMain) {
          event.preventDefault()
          event.stopPropagation()
          return
        }
      }

      if (editingField) {
        if (isBackKey) {
          setEditingField(null)
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      if (inMain && isBackKey) {
        const ok = focusSelectedNav()
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      // ENTER/selectDown: Switch/Dropdown/Button → aktivieren; Formfelder → Edit-Mode
      if (inMain && (isEnter || isSelectDown)) {
        const role = active?.getAttribute('role') || ''
        const tag = active?.tagName || ''

        const isSwitch =
          role === 'switch' || (tag === 'INPUT' && (active as HTMLInputElement).type === 'checkbox')

        const isDropdown =
          role === 'combobox' && active?.getAttribute('aria-haspopup') === 'listbox'

        if (isSwitch || isDropdown || role === 'button') {
          const ok = activateControl(active)
          if (ok) {
            event.preventDefault()
            event.stopPropagation()
            return
          }
        }

        if (formFocused) {
          setEditingField(active!)
          if (active?.tagName === 'INPUT' && (active as HTMLInputElement).type === 'number') {
            ;(active as HTMLInputElement).select()
          }
          event.preventDefault()
          event.stopPropagation()
          return
        }

        const ok = activateControl(active || null)
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
          return
        }
      }

      // Pfeilnavigation linear (DOM-Reihenfolge)
      if (inMain && (isLeft || isUp)) {
        const ok = moveFocusLinear(-1)
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }
      if (inMain && (isRight || isDown)) {
        const ok = moveFocusLinear(1)
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      const isTransport =
        code === b?.next ||
        code === b?.prev ||
        code === b?.play ||
        code === b?.pause ||
        code === b?.seekFwd ||
        code === b?.seekBack

      if (!isCarPlayActive && isTransport) {
        const action: BindKey =
          code === b?.next
            ? 'next'
            : code === b?.prev
              ? 'prev'
              : code === b?.play
                ? 'play'
                : code === b?.pause
                  ? 'pause'
                  : code === b?.seekFwd
                    ? 'seekFwd'
                    : 'seekBack'
        setKeyCommand(action as KeyCommand)
        setCommandCounter((p) => p + 1)
        broadcastMediaKey(action)
      }

      if ((isLeft || isRight || isDown) && nothing) {
        const ok = focusSelectedNav()
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }
    },
    [
      settings,
      location.pathname,
      receivingVideo,
      inContainer,
      focusSelectedNav,
      focusFirstInMain,
      moveFocusLinear,
      isFormField,
      editingField,
      activateControl
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
