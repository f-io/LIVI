import { clamp } from '@renderer/utils'
import * as React from 'react'
import { useCallback, useRef } from 'react'
import { usePaginationDots } from '@renderer/components/pages/telemetry/hooks/usePaginationDots'

export const useKeyboardNavigation = ({
  dashboards,
                                        index,
                                        isNavbarHidden,
  onSetIndex
}) => {
  // swipe
  const startRef = useRef<{ x: number; y: number; t: number } | null>(null)
  const pagerStateRef = useRef({ index: 0, len: 0 })

  const {
    revealDots
  } = usePaginationDots(isNavbarHidden)

  const handleNavigate = useCallback(
    (dir: -1 | 1) => {
      if (dashboards.length <= 1) return
      onSetIndex((prev) => clamp(prev + dir, 0, dashboards.length - 1))
      revealDots()
    },
    [dashboards.length, onSetIndex, revealDots]
  )

  const handlePointerUp = useCallback((e: React.PointerEvent) => {
    if (!e.isPrimary) return
    const start = startRef.current
    startRef.current = null
    if (!start) return

    const dx = e.clientX - start.x
    const dy = e.clientY - start.y
    const dt = performance.now() - start.t

    const absX = Math.abs(dx)
    const absY = Math.abs(dy)

    if (absX < 40) return
    if (absY > absX * 0.8) return
    if (dt > 900) return

    if (dx < 0)
      handleNavigate(1) // swipe left -> next
    else handleNavigate(-1) // swipe right -> prev
  }, [handleNavigate])

  const handlePointerDown = useCallback((e: React.PointerEvent) => {
    if (!e.isPrimary) return
    startRef.current = { x: e.clientX, y: e.clientY, t: performance.now() }
  }, [])

  // ---- register pager for global key handler (useKeyDown) ----
  pagerStateRef.current = { index, len: dashboards.length }

  const prev = useCallback(() => {
    const { index: i } = pagerStateRef.current
    if (i <= 0) return
    handleNavigate(-1)
  }, [handleNavigate])

  const next = useCallback(() => {
    const { index: i, len } = pagerStateRef.current
    if (len <= 0) return
    if (i >= len - 1) return
    handleNavigate(1)
  }, [handleNavigate])

  const canPrev = useCallback(() => pagerStateRef.current.index > 0, [])
  const canNext = useCallback(() => {
    const { index: i, len } = pagerStateRef.current
    return len > 0 && i < len - 1
  }, [])

  return {
    prev,
    next,
    canPrev,
    canNext,
    onNavigate: handleNavigate,
    onPointerDown: handlePointerDown,
    onPointerUp: handlePointerUp
  }
}
