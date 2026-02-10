import { useCallback, useContext, useMemo } from 'react'
import { BindKey, useKeyDownProps } from './types'
import { broadcastMediaKey } from '../../utils/broadcastMediaKey'
import { KeyCommand } from '../../components/worker/types'
import { useLocation } from 'react-router'
import { ROUTES } from '../../constants'
import { AppContext } from '../../context'
import { useCarplayStore } from '@store/store'

type RefLike<T> = { current: T | null }

function isRefLike<T>(v: unknown): v is RefLike<T> {
  return typeof v === 'object' && v !== null && 'current' in v
}

function readRefCurrent<T>(v: unknown): T | null {
  return isRefLike<T>(v) ? v.current : null
}

export const useKeyDown = ({
  receivingVideo,
  inContainer,
  focusSelectedNav,
  focusFirstInMain,
  moveFocusLinear,
  isFormField,
  activateControl,
  onSetKeyCommand,
  onSetCommandCounter
}: useKeyDownProps) => {
  const location = useLocation()

  const currentRoute = useMemo(() => {
    // HashRouter: "#/media" -> "/media", "#media" -> "/media"
    const raw = location.hash ? location.hash.replace(/^#/, '') : ''
    if (raw) return raw.startsWith('/') ? raw : `/${raw}`

    // BrowserRouter fallback
    return location.pathname || '/'
  }, [location.hash, location.pathname])

  const appContext = useContext(AppContext)
  const settings = useCarplayStore((s) => s.settings)

  const navRef = appContext?.navEl
  const mainRef = appContext?.contentEl

  const editingField = appContext?.keyboardNavigation?.focusedElId

  const handleSetFocusedElId = useCallback(
    (active: HTMLElement | null) => {
      const elementId = active?.id || active?.getAttribute('aria-label') || null
      const currentFocusedElementId = appContext?.keyboardNavigation?.focusedElId

      if (elementId === null) {
        appContext?.onSetAppContext?.({
          ...appContext,
          keyboardNavigation: {
            focusedElId: null
          }
        })
        return
      }

      appContext?.onSetAppContext?.({
        ...appContext,
        keyboardNavigation: {
          focusedElId: currentFocusedElementId === elementId ? null : elementId
        }
      })
    },
    [appContext]
  )

  return useCallback(
    (event: KeyboardEvent) => {
      const code = event.code
      const active = document.activeElement as HTMLElement | null
      const isCarPlayRouteActive = currentRoute === ROUTES.HOME
      const isCarPlayActive = isCarPlayRouteActive && receivingVideo

      const b = (settings?.bindings ?? {}) as Partial<Record<BindKey, string>>

      const leftKey = b?.left || 'ArrowLeft'
      const rightKey = b?.right || 'ArrowRight'
      const upKey = b?.up || 'ArrowUp'
      const downKey = b?.down || 'ArrowDown'

      const isLeft = code === leftKey
      const isRight = code === rightKey
      const isUp = code === upKey
      const isDown = code === downKey

      const isBackKey = code === (b?.back || '') || code === 'Escape'
      const isEnter = code === 'Enter' || code === 'NumpadEnter'
      const isSelectDown = code === (b?.selectDown || '') || isEnter

      let mappedAction: BindKey | undefined
      for (const [k, v] of Object.entries(b ?? {})) {
        if (v === code) {
          mappedAction = k as BindKey
          break
        }
      }

      const navRoot = readRefCurrent<HTMLElement>(navRef) ?? document.getElementById('nav-root')
      const mainRoot =
        readRefCurrent<HTMLElement>(mainRef) ?? document.getElementById('content-root')

      const inNav = inContainer(navRoot, active) || !!active?.closest?.('#nav-root')
      const inMain = inContainer(mainRoot, active) || !!active?.closest?.('#content-root')

      const nothing = !active || active === document.body
      const formFocused = isFormField(active)

      if (formFocused && !editingField && code === 'Backspace') {
        return
      }

      if (settings && isCarPlayActive && mappedAction && !inNav) {
        onSetKeyCommand(mappedAction as KeyCommand)
        onSetCommandCounter((p) => p + 1)
        broadcastMediaKey(mappedAction)
        if (mappedAction === 'selectDown') {
          setTimeout(() => {
            onSetKeyCommand('selectUp' as KeyCommand)
            onSetCommandCounter((p) => p + 1)
            broadcastMediaKey('selectUp')
          }, 200)
        }
        event.preventDefault()
        event.stopPropagation()
        return
      }

      if (inNav) {
        if (isEnter || isSelectDown) {
          const target = document.activeElement as HTMLElement | null

          const ok = activateControl(target)
          if (!ok && target) {
            target.click()
          }

          event.preventDefault()
          event.stopPropagation()
          return
        }

        if (isRight) {
          requestAnimationFrame(() => {
            focusFirstInMain()
          })
          event.preventDefault()
          event.stopPropagation()
          return
        }
      }

      {
        const wantEnterMainFromNav = inNav && (isRight || isEnter || isSelectDown)
        const wantEnterMainFromNothing =
          nothing && (isLeft || isRight || isUp || isDown || isEnter || isSelectDown)

        if (currentRoute !== ROUTES.HOME && (wantEnterMainFromNav || wantEnterMainFromNothing)) {
          const okMain = focusFirstInMain()
          if (okMain) {
            event.preventDefault()
            event.stopPropagation()
            return
          }
        }
      }

      const isInputOrEditable = (_active: HTMLElement | null) =>
        _active?.tagName === 'INPUT' ||
        _active?.tagName === 'TEXTAREA' ||
        _active?.getAttribute('contenteditable') === 'true' ||
        _active?.getAttribute('role') === 'slider' ||
        _active?.getAttribute('role') === 'switch' ||
        (_active instanceof HTMLInputElement && _active.type === 'range') ||
        (_active instanceof HTMLInputElement && _active.type === 'listbox')

      const isRangeSlider =
        (active?.tagName === 'INPUT' && (active as HTMLInputElement).type === 'range') ||
        active?.getAttribute('role') === 'slider'

      if (inMain && isBackKey) {
        const activeNow = document.activeElement as HTMLElement | null

        if (editingField) {
          const isRangeInput =
            activeNow?.tagName === 'INPUT' && (activeNow as HTMLInputElement).type === 'range'

          if (!isRangeInput) {
            handleSetFocusedElId(null)
            event.preventDefault()
            event.stopPropagation()
            return
          }
        }

        const isSettingsRoute = currentRoute.startsWith(ROUTES.SETTINGS)
        const isSettingsRoot = currentRoute === ROUTES.SETTINGS

        if (isSettingsRoute && !isSettingsRoot) {
          window.history.back()
          event.preventDefault()
          event.stopPropagation()
          return
        }

        const ok = focusSelectedNav()
        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      if (inMain && (isEnter || isSelectDown)) {
        const colorInput =
          (active instanceof HTMLInputElement && active.type === 'color'
            ? active
            : (active?.querySelector?.('input[type="color"]') as HTMLInputElement | null)) ??
          (active
            ?.closest?.('[role="button"]')
            ?.querySelector?.('input[type="color"]') as HTMLInputElement | null)

        if (colorInput) {
          colorInput.focus()
          colorInput.click()
          event.preventDefault()
          event.stopPropagation()
          return
        }

        const role = active?.getAttribute('role') || ''
        const tag = active?.tagName || ''

        const isSwitch =
          role === 'switch' || (tag === 'INPUT' && (active as HTMLInputElement).type === 'checkbox')

        const isDropdown =
          role === 'combobox' && active?.getAttribute('aria-haspopup') === 'listbox'

        const isSlider = tag === 'INPUT' && (active as HTMLInputElement).type === 'range'

        if (isSwitch || isDropdown || role === 'button') {
          if (!isSlider) {
            const ok = activateControl(active)
            if (ok) {
              event.preventDefault()
              event.stopPropagation()

              if (isDropdown) {
                handleSetFocusedElId(active)
              }
              return
            }
          }
        }

        if (formFocused) {
          if (editingField) {
            handleSetFocusedElId(null)

            event.preventDefault()
            event.stopPropagation()

            return
          }
          handleSetFocusedElId(active)

          if (
            active?.tagName === 'INPUT' &&
            ['number', 'range'].includes((active as HTMLInputElement).type)
          ) {
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

      if (inMain && (isLeft || isUp)) {
        if (isRangeSlider && isLeft) {
          return
        }

        if (
          !isRangeSlider &&
          editingField &&
          isInputOrEditable(document.activeElement as HTMLElement)
        ) {
          return
        }

        const ok = moveFocusLinear(-1)

        if (isRangeSlider && isUp) {
          event.preventDefault()
          event.stopPropagation()
          return
        }

        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      if (inMain && (isRight || isDown)) {
        if (isRangeSlider && isRight) {
          return
        }

        if (
          !isRangeSlider &&
          editingField &&
          isInputOrEditable(document.activeElement as HTMLElement)
        ) {
          return
        }

        const ok = moveFocusLinear(1)

        if (isRangeSlider && isDown) {
          event.preventDefault()
          event.stopPropagation()
          return
        }

        if (ok) {
          event.preventDefault()
          event.stopPropagation()
        }
        return
      }

      const isTransport =
        code === b?.next ||
        code === b?.prev ||
        code === b?.playPause ||
        code === b?.play ||
        code === b?.pause ||
        code === b?.acceptPhone ||
        code === b?.rejectPhone ||
        code === b?.siri

      if (settings && !isCarPlayActive && isTransport) {
        const action: KeyCommand =
          code === b?.next
            ? 'next'
            : code === b?.prev
              ? 'prev'
              : code === b?.playPause
                ? 'playPause'
                : code === b?.play
                  ? 'play'
                  : code === b?.pause
                    ? 'pause'
                    : code === b?.acceptPhone
                      ? 'acceptPhone'
                      : code === b?.rejectPhone
                        ? 'rejectPhone'
                        : 'siri'

        onSetKeyCommand(action)
        onSetCommandCounter((p) => p + 1)
        broadcastMediaKey(action)
      }

      if ((isLeft || isRight || isUp || isDown) && nothing) {
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
      currentRoute,
      receivingVideo,
      inContainer,
      navRef,
      mainRef,
      isFormField,
      editingField,
      onSetKeyCommand,
      onSetCommandCounter,
      focusFirstInMain,
      focusSelectedNav,
      handleSetFocusedElId,
      activateControl,
      moveFocusLinear
    ]
  )
}
