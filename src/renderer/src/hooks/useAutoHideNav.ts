import { useCallback, useEffect, useRef, useState } from 'react'
import { UI } from '../constants'

// Auto-hide a nav rail after a period of inactivity. Wakes on keydown,
// mousemove, wheel, focusin (within an optional containerEl). When `enabled`
// flips false the nav becomes visible immediately and any pending hide timer
// is cleared. Pure React state — works in both AppLayout (router-based) and
// secondary windows (no router).
export function useAutoHideNav(enabled: boolean, containerEl?: HTMLElement | null) {
  const [hidden, setHidden] = useState(false)
  const hideTimerRef = useRef<number | null>(null)

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current != null) {
      window.clearTimeout(hideTimerRef.current)
      hideTimerRef.current = null
    }
  }, [])

  const scheduleHide = useCallback(() => {
    clearHideTimer()
    hideTimerRef.current = window.setTimeout(() => {
      setHidden(true)
      hideTimerRef.current = null
    }, UI.INACTIVITY_HIDE_DELAY_MS)
  }, [clearHideTimer])

  const wake = useCallback(() => {
    setHidden(false)
    if (enabled) scheduleHide()
  }, [enabled, scheduleHide])

  useEffect(() => {
    if (!enabled) {
      clearHideTimer()
      setHidden(false)
      return
    }

    setHidden(false)
    scheduleHide()
    return clearHideTimer
  }, [enabled, scheduleHide, clearHideTimer])

  useEffect(() => {
    if (!enabled) return

    const onActivity: EventListener = () => wake()
    const onFocusIn: EventListener = () => {
      if (!containerEl) return wake()
      const active = document.activeElement as HTMLElement | null
      if (active && containerEl.contains(active)) wake()
    }

    window.addEventListener('keydown', onActivity, { passive: true })
    document.addEventListener('mousemove', onActivity, { passive: true })
    document.addEventListener('wheel', onActivity, { passive: true })
    document.addEventListener('focusin', onFocusIn)

    return () => {
      window.removeEventListener('keydown', onActivity)
      document.removeEventListener('mousemove', onActivity)
      document.removeEventListener('wheel', onActivity)
      document.removeEventListener('focusin', onFocusIn)
    }
  }, [enabled, wake, containerEl])

  return { hidden, wake }
}
