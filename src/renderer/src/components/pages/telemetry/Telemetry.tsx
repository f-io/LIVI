import * as React from 'react'
import { Box, useTheme } from '@mui/material'
import { useCarplayStore } from '../../../store/store'
import { AppContext } from '../../../context'

import type { TelemetryDashboardConfig, TelemetryDashboardId } from '@main/Globals'

// Dashboards
import { Dash1 } from './dashboards/Dash1'
import { Dash2 } from './dashboards/Dash2'
import { Dash3 } from './dashboards/Dash3'
import { Dash4 } from './dashboards/Dash4'

// Placeholder
import { DashPlaceholder } from './components/DashPlaceholder'

const isDashId = (id: unknown): id is TelemetryDashboardId =>
  id === 'dash1' || id === 'dash2' || id === 'dash3' || id === 'dash4'

type DashPage = {
  id: TelemetryDashboardId
  pos: number
  Component: React.ComponentType
}

const getDashComponent = (id: TelemetryDashboardId): React.ComponentType => {
  switch (id) {
    case 'dash4':
      return Dash4
    case 'dash1':
      return Dash1
    case 'dash2':
      return Dash2
    case 'dash3':
      return Dash3
    default:
      return () => <DashPlaceholder title="Unknown dash" />
  }
}

const clamp = (n: number, min: number, max: number) => Math.max(min, Math.min(max, n))

export const Telemetry: React.FC = () => {
  const theme = useTheme()
  const settings = useCarplayStore((s) => s.settings)

  const { onSetAppContext } = React.useContext(AppContext)

  const pages = React.useMemo<DashPage[]>(() => {
    const raw = settings?.telemetryDashboards
    const base: TelemetryDashboardConfig[] = Array.isArray(raw) ? raw : []

    const enabled = base
      .filter((d) => d && isDashId(d.id) && Boolean(d.enabled))
      .map((d) => ({
        id: d.id,
        pos: typeof d.pos === 'number' && Number.isFinite(d.pos) ? Math.round(d.pos) : 9999
      }))
      .sort((a, b) => a.pos - b.pos)

    // stable normalize positions
    const normalized = enabled.map((d, idx) => ({ ...d, pos: idx + 1 }))

    return normalized.map((d) => ({
      id: d.id,
      pos: d.pos,
      Component: getDashComponent(d.id)
    }))
  }, [settings?.telemetryDashboards])

  const [index, setIndex] = React.useState(0)
  const [showDots, setShowDots] = React.useState(false)
  const [navHidden, setNavHidden] = React.useState(() => {
    const el = document.getElementById('content-root')
    return el?.getAttribute('data-nav-hidden') === '1'
  })

  React.useLayoutEffect(() => {
    const el = document.getElementById('content-root')
    if (!el) return

    const read = () => setNavHidden(el.getAttribute('data-nav-hidden') === '1')
    read()

    const mo = new MutationObserver(read)
    mo.observe(el, { attributes: true, attributeFilter: ['data-nav-hidden'] })

    return () => mo.disconnect()
  }, [])

  // keep index valid if pages change
  React.useEffect(() => {
    setIndex((prev) => clamp(prev, 0, Math.max(0, pages.length - 1)))
  }, [pages.length])

  // dots timer
  const dotsTimerRef = React.useRef<number | null>(null)
  const revealDots = React.useCallback(() => {
    setShowDots(true)
    if (dotsTimerRef.current != null) window.clearTimeout(dotsTimerRef.current)
    dotsTimerRef.current = window.setTimeout(() => setShowDots(false), 2000)
  }, [])

  React.useEffect(() => {
    return () => {
      if (dotsTimerRef.current != null) window.clearTimeout(dotsTimerRef.current)
    }
  }, [])

  React.useEffect(() => {
    if (!navHidden) {
      setShowDots(true)
      if (dotsTimerRef.current != null) window.clearTimeout(dotsTimerRef.current)
      dotsTimerRef.current = null
      return
    }

    setShowDots(false)
  }, [navHidden])

  const go = React.useCallback(
    (dir: -1 | 1) => {
      if (pages.length <= 1) return
      setIndex((prev) => clamp(prev + dir, 0, pages.length - 1))
      revealDots()
    },
    [pages.length, revealDots]
  )

  // ---- register pager for global key handler (useKeyDown) ----
  const pagerStateRef = React.useRef({ index: 0, len: 0 })
  pagerStateRef.current = { index, len: pages.length }

  const prev = React.useCallback(() => {
    const { index: i } = pagerStateRef.current
    if (i <= 0) return
    go(-1)
  }, [go])

  const next = React.useCallback(() => {
    const { index: i, len } = pagerStateRef.current
    if (len <= 0) return
    if (i >= len - 1) return
    go(1)
  }, [go])

  const canPrev = React.useCallback(() => pagerStateRef.current.index > 0, [])
  const canNext = React.useCallback(() => {
    const { index: i, len } = pagerStateRef.current
    return len > 0 && i < len - 1
  }, [])

  React.useEffect(() => {
    if (!onSetAppContext) return

    // register (PATCH only!)
    onSetAppContext({
      telemetryPager: { prev, next, canPrev, canNext }
    })

    // cleanup on unmount
    return () => {
      onSetAppContext({
        telemetryPager: undefined
      })
    }
  }, [onSetAppContext, prev, next, canPrev, canNext])

  // swipe
  const startRef = React.useRef<{ x: number; y: number; t: number } | null>(null)

  const onPointerDown = (e: React.PointerEvent) => {
    if (!e.isPrimary) return
    startRef.current = { x: e.clientX, y: e.clientY, t: performance.now() }
  }

  const onPointerUp = (e: React.PointerEvent) => {
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
      go(1) // swipe left -> next
    else go(-1) // swipe right -> prev
  }

  // render
  if (pages.length === 0) {
    return (
      <Box
        sx={{
          position: 'absolute',
          inset: 0,
          backgroundColor: theme.palette.background.default,
          display: 'grid',
          placeItems: 'center',
          opacity: 0.8
        }}
      >
        <DashPlaceholder title="No dashboards enabled" />
      </Box>
    )
  }

  const Active = pages[index]?.Component ?? (() => <DashPlaceholder title="Missing page" />)

  return (
    <Box
      sx={{
        position: navHidden ? 'fixed' : 'absolute',
        inset: 0,
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        backgroundColor: theme.palette.background.default
      }}
      onPointerDown={onPointerDown}
      onPointerUp={onPointerUp}
    >
      <Active />

      {/* dots overlay */}
      {pages.length > 1 && (
        <Box
          sx={{
            position: 'absolute',
            left: 0,
            right: 0,
            bottom: 'clamp(10px, 2.2svh, 18px)',
            display: 'flex',
            justifyContent: 'center',
            pointerEvents: 'auto',
            opacity: showDots ? 1 : 0,
            transition: 'opacity 180ms ease-out'
          }}
        >
          <Box
            sx={{
              display: 'flex',
              gap: 'clamp(6px, 1.2svh, 10px)',
              px: 'clamp(10px, 2svh, 14px)',
              py: 'clamp(6px, 1.4svh, 10px)',
              borderRadius: 999,
              backgroundColor:
                theme.palette.mode === 'dark' ? 'rgba(0,0,0,0.35)' : 'rgba(0,0,0,0.18)',
              backdropFilter: 'blur(6px)'
            }}
          >
            {pages.map((p, i) => (
              <Box
                key={p.id}
                role="button"
                tabIndex={0}
                onPointerDown={(e) => {
                  e.preventDefault()
                  e.stopPropagation()
                  setIndex(i)
                  revealDots()
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault()
                    e.stopPropagation()
                    setIndex(i)
                    revealDots()
                  }
                }}
                sx={{
                  width: 'clamp(14px, 2.6svh, 22px)',
                  height: 'clamp(14px, 2.6svh, 22px)',
                  display: 'grid',
                  placeItems: 'center',
                  borderRadius: 999,
                  cursor: 'pointer',
                  WebkitTapHighlightColor: 'transparent'
                }}
              >
                <Box
                  sx={{
                    width: 'clamp(6px, 1.2svh, 10px)',
                    height: 'clamp(6px, 1.2svh, 10px)',
                    borderRadius: 999,
                    backgroundColor:
                      i === index ? theme.palette.primary.main : theme.palette.text.secondary,
                    opacity: i === index ? 1 : 0.45,
                    transition: 'opacity 120ms ease-out'
                  }}
                />
              </Box>
            ))}
          </Box>
        </Box>
      )}
    </Box>
  )
}
