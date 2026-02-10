import { useCallback, useContext } from 'react'
import { AppContext } from '../../context'
import { FOCUSABLE_SELECTOR } from '../../constants'

type RefLike<T> = { current: T | null }

function isRefLike<T>(v: unknown): v is RefLike<T> {
  return typeof v === 'object' && v !== null && 'current' in v
}

function readRefCurrent<T>(v: unknown): T | null {
  return isRefLike<T>(v) ? v.current : null
}

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

      const seed = root.querySelector<HTMLElement>('[data-seed="first"]')
      if (seed && list.includes(seed)) return seed

      const nonForm = list.find((el) => !isFormField(el))

      return nonForm ?? list[0]
    },
    [getFocusableList, isFormField]
  )

  const focusSelectedNav = useCallback(() => {
    const navRoot = readRefCurrent<HTMLElement>(navRef) ?? document.getElementById('nav-root')

    if (!navRoot) return false

    const target =
      (navRoot.querySelector('[role="tab"][aria-selected="true"]') as HTMLElement | null) ||
      getFirstFocusable(navRoot)

    if (!target) return false

    target.focus({ preventScroll: true })

    return document.activeElement === target
  }, [getFirstFocusable, navRef])

  const focusFirstInMain = useCallback(() => {
    const mainRoot = readRefCurrent<HTMLElement>(mainRef) ?? document.getElementById('content-root')

    if (!mainRoot) return false

    const target = getFirstFocusable(mainRoot)
    if (!target) return false

    target.focus({ preventScroll: true })

    return document.activeElement === target
  }, [getFirstFocusable, mainRef])

  const moveFocusLinear = useCallback(
    (delta: -1 | 1) => {
      const mainRoot =
        readRefCurrent<HTMLElement>(mainRef) ?? document.getElementById('content-root')

      const list = getFocusableList(mainRoot)

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
        const scrolledWrapper = mainRoot?.querySelector(
          '[data-scrolled-wrapper]'
        ) as HTMLElement | null

        if (scrolledWrapper && !scrolledWrapper.contains(next)) {
          scrolledWrapper.scrollTop = 0
        }

        if (scrolledWrapper) {
          const nr = next.getBoundingClientRect()
          const wr = scrolledWrapper.getBoundingClientRect()

          if (nr.top < wr.top) {
            scrolledWrapper.scrollTop -= wr.top - nr.top
          } else if (nr.bottom > wr.bottom) {
            scrolledWrapper.scrollTop += nr.bottom - wr.bottom
          }
        } else {
          next.scrollIntoView({ block: 'nearest' })
        }

        next.focus({ preventScroll: true })

        appContext?.onSetAppContext?.({
          ...appContext,
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
