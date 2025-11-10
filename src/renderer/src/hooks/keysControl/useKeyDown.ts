import { useCallback } from 'react'
import { BindKey, useKeyDownProps } from './types'
import { broadcastMediaKey } from '../../utils/broadcastMediaKey'
import { KeyCommand } from '../../components/worker/types'
import { useLocation } from 'react-router-dom'
import { ROUTES } from '../../constants'

// TODO
// change function naming by react standards
// create enums & constants for all key codes
// split hook to separate hooks by the logic

export const useKeyDown = ({
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
  onSetKeyCommand,
  onSetCommandCounter,
  onSetEditingField
}: useKeyDownProps) => {
  const location = useLocation()

  return useCallback(
    (event: KeyboardEvent) => {
      if (!settings) return

      const code = event.code
      const active = document.activeElement as HTMLElement | null
      const isCarPlayActive = location.pathname === ROUTES.HOME && receivingVideo

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

      if (location.pathname !== ROUTES.HOME && nothing && (isLeft || isRight || isUp || isDown)) {
        const okMain = focusFirstInMain()

        if (okMain) {
          event.preventDefault()
          event.stopPropagation()
          return
        }
      }

      // TODO - should be triggered when the field is already empty and the user clicks backspace again
      if (editingField) {
        if (isBackKey) {
          onSetEditingField(null)
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
          onSetEditingField(active!)
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
        onSetKeyCommand(action as KeyCommand)
        onSetCommandCounter((p) => p + 1)
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
      navRef,
      mainRef,
      isFormField,
      editingField,
      onSetKeyCommand,
      onSetCommandCounter,
      focusFirstInMain,
      onSetEditingField,
      focusSelectedNav,
      activateControl,
      moveFocusLinear
    ]
  )
}
