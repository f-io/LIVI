import { useEffect, useState } from 'react'

type SessionEvent = { type?: string; position?: number; total?: number }
type Bridge = {
  projection?: { ipc?: { onEvent?: (cb: (e: unknown, ...a: unknown[]) => void) => () => void } }
}

export function SessionSwitchOverlay() {
  const [content, setContent] = useState<{ position: number; total: number; tick: number } | null>(
    null
  )

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const ev = (args[0] ?? {}) as SessionEvent
      if (ev?.type !== 'session') return
      const position = typeof ev.position === 'number' ? ev.position : 0
      const total = typeof ev.total === 'number' ? ev.total : 0
      if (position < 1) return
      setContent((prev) => ({ position, total, tick: (prev?.tick ?? 0) + 1 }))
    }

    const w = window as unknown as Bridge
    let unsubscribe: (() => void) | undefined
    if (typeof w.projection?.ipc?.onEvent === 'function') {
      const maybe = w.projection.ipc.onEvent(handler)
      if (typeof maybe === 'function') unsubscribe = maybe
    }
    return () => {
      if (typeof unsubscribe === 'function') {
        try {
          unsubscribe()
        } catch {}
      }
    }
  }, [])

  if (!content) return null

  return (
    <>
      <style>
        {'@keyframes liviSessionSwitch{0%{opacity:0}12%{opacity:1}78%{opacity:1}100%{opacity:0}}'}
      </style>
      <div
        key={content.tick}
        aria-hidden
        style={{
          position: 'fixed',
          top: 16,
          right: 16,
          zIndex: 4000,
          pointerEvents: 'none',
          padding: '10px 20px',
          borderRadius: 14,
          background: 'rgba(18, 18, 20, 0.72)',
          color: 'rgba(255, 255, 255, 0.92)',
          fontSize: 28,
          fontWeight: 500,
          lineHeight: 1,
          letterSpacing: '0.02em',
          whiteSpace: 'nowrap',
          boxShadow: '0 6px 22px rgba(0, 0, 0, 0.4)',
          opacity: 0,
          animation: 'liviSessionSwitch 1500ms ease-in-out forwards'
        }}
      >
        <span style={{ color: 'var(--ui-highlight, #00e5ff)' }}>{content.position}</span>/
        {content.total}
      </div>
    </>
  )
}

export default SessionSwitchOverlay
