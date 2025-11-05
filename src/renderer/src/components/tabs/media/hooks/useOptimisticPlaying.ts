// Optimistic play/pause with auto-reconcile
import { useEffect, useRef, useState } from 'react'

export function useOptimisticPlaying(realPlaying: boolean | undefined) {
  const [override, setOverride] = useState<boolean | null>(null)
  const timer = useRef<number | null>(null)

  useEffect(() => {
    if (override === null) return
    if (typeof realPlaying === 'boolean' && realPlaying === override) {
      if (timer.current) window.clearTimeout(timer.current)
      setOverride(null)
    }
  }, [realPlaying, override])

  useEffect(() => {
    if (override === null) return
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => setOverride(null), 1500)
    return () => {
      if (timer.current) window.clearTimeout(timer.current)
    }
  }, [override])

  const uiPlaying = override ?? !!realPlaying
  return { uiPlaying, setOverride, clearOverride: () => setOverride(null) }
}
