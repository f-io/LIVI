import { useCallback, useContext } from 'react'
import { AppContext } from '../../context'
import { FOCUSABLE_SELECTOR } from '../../constants'

export const useFocus = () => {
  const appContext = useContext(AppContext)

  const navRef = appContext.navEl
  const mainRef = appContext.contentEl

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
    (root?: HTMLElement | null): HTMLElement[] => {
      if (!root) return []

      const all = Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))

      return all.filter(isVisible).filter((el) => !el.closest('[aria-hidden="true"], [inert]'))
    },
    [isVisible]
  )

  const getFirstFocusable = useCallback(
    (root?: HTMLElement | null): HTMLElement | null => {
      if (!root) return null
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
      (navRef?.current?.querySelector('[role="tab"][aria-selected="true"]') as HTMLElement) ||
      getFirstFocusable(navRef?.current)
    target?.focus({ preventScroll: true })

    return !!target
  }, [getFirstFocusable, navRef])

  const focusFirstInMain = useCallback(() => {
    const target = getFirstFocusable(mainRef?.current)

    target?.focus({ preventScroll: true })

    return !!target
  }, [getFirstFocusable, mainRef])

  const moveFocusLinear = useCallback(
    (delta: -1 | 1) => {
      const list = getFocusableList(mainRef?.current)

      if (!list.length) return false

      const active = (document.activeElement as HTMLElement | null) ?? null
      let next: HTMLElement | null = null

      if (!active || !list.includes(active)) {
        next = delta > 0 ? list[0] : list[list.length - 1]
      } else {
        const idx = list.indexOf(active)
        const targetIdx = idx + delta
        if (targetIdx >= 0 && targetIdx < list.length) next = list[targetIdx]

        if (targetIdx <= 1) {
          const scrolledWrapper = mainRef?.current?.querySelector('[data-scrolled-wrapper]')

          scrolledWrapper?.scrollTo(0, 0)
        }
      }

      if (next) {
        next.focus()

        appContext?.onSetAppContext?.({
          keyboardNavigation: {
            focusedElId: null
          }
        })
        return true
      }

      return false
    },
    [appContext, getFocusableList, mainRef]
  )

  return {
    isVisible,
    isFormField,
    getFocusableList,
    getFirstFocusable,
    focusSelectedNav,
    focusFirstInMain,
    moveFocusLinear
  }
}
