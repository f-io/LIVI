import { useCallback, useEffect, useRef, useState } from 'react'

export const usePaginationDots = (isNavbarHidden: boolean) => {
  const [showDots, setShowDots] = useState(false)

  const dotsTimerRef = useRef<number | null>(null)
  const revealDots = useCallback(() => {
    setShowDots(true)
    if (dotsTimerRef.current != null) window.clearTimeout(dotsTimerRef.current)
    dotsTimerRef.current = window.setTimeout(() => setShowDots(false), 2000)
  }, [])

  useEffect(() => {
    return () => {
      if (dotsTimerRef.current != null) window.clearTimeout(dotsTimerRef.current)
    }
  }, [])

  useEffect(() => {
    if (!isNavbarHidden) {
      setShowDots(true)
      if (dotsTimerRef.current != null) window.clearTimeout(dotsTimerRef.current)
      dotsTimerRef.current = null
      return
    }

    setShowDots(false)
  }, [isNavbarHidden])

  return {
    showDots,
    revealDots
  }
}
