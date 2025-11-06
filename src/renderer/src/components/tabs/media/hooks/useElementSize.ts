import { useEffect, useRef, useState } from 'react'

export function useElementSize<T extends HTMLElement>() {
  const ref = useRef<T | null>(null)
  const [size, set] = useState({ w: window.innerWidth, h: window.innerHeight })
  const rafRef = useRef<number | null>(null)
  const pendingRef = useRef<{ w: number; h: number } | null>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return

    const flush = () => {
      rafRef.current = null
      const next = pendingRef.current
      pendingRef.current = null
      if (!next) return
      set((prev) => (prev.w !== next.w || prev.h !== next.h ? next : prev))
    }

    const ro = new ResizeObserver((entries) => {
      const r = entries[0]?.contentRect
      if (!r) return
      // Round to avoid sub-pixel churn
      pendingRef.current = { w: Math.round(r.width), h: Math.round(r.height) }
      if (rafRef.current == null) {
        rafRef.current = requestAnimationFrame(flush)
      }
    })

    ro.observe(el)
    return () => {
      ro.disconnect()
      if (rafRef.current != null) cancelAnimationFrame(rafRef.current)
    }
  }, [])

  return [ref, size] as const
}
