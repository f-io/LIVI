import { useEffect, useMemo, useRef, useState } from 'react'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import PauseIcon from '@mui/icons-material/Pause'
import SkipNextIcon from '@mui/icons-material/SkipNext'
import SkipPreviousIcon from '@mui/icons-material/SkipPrevious'
import { useStatusStore } from '../../../store/store'

// Types
type PersistedSnapshot = { timestamp: string; payload: MediaPayload }
type MediaPayload = {
  type: number
  media?: {
    MediaSongName?: string
    MediaAlbumName?: string
    MediaArtistName?: string
    MediaAPPName?: string
    MediaSongDuration?: number
    MediaSongPlayTime?: number
    MediaPlayStatus?: number
    MediaLyrics?: string
  }
  base64Image?: string
}

// USB/carplay event shape
type UsbEvent = { type?: string } & Record<string, unknown>

// Utils
function msToClock(ms?: number): string {
  if (!ms || ms < 0) return '0:00'
  const total = Math.floor(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${s.toString().padStart(2, '0')}`
}
function clamp(n: number, min: number, max: number) {
  return Math.max(min, Math.min(max, n))
}
function mergePayload(prev: MediaPayload | undefined, inc: MediaPayload): MediaPayload {
  const prevMedia = prev?.media ?? {}
  const incMedia = inc.media ?? {}
  return {
    type: inc.type ?? prev?.type ?? 0,
    media:
      Object.keys(prevMedia).length || Object.keys(incMedia).length
        ? { ...prevMedia, ...incMedia }
        : undefined,
    base64Image: inc.base64Image !== undefined ? inc.base64Image : prev?.base64Image
  }
}
type MediaEventPayload = { type: 'media'; payload: { payload: MediaPayload } }
function payloadFromLiveEvent(ev: unknown): MediaPayload | null {
  const e = ev as Partial<MediaEventPayload>
  if (e?.type !== 'media' || !e.payload?.payload) return null
  return e.payload.payload
}

// Hooks
function useElementSize<T extends HTMLElement>() {
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

function useBelowNavTop() {
  const [top, setTop] = useState(0)
  useEffect(() => {
    const getTop = () => {
      const nav = document.querySelector('.MuiTabs-root') as HTMLElement | null
      const t = nav ? nav.getBoundingClientRect().bottom : 0
      setTop(Math.max(0, Math.round(t)))
    }
    getTop()
    const onResize = () => getTop()
    window.addEventListener('resize', onResize)
    const nav = document.querySelector('.MuiTabs-root') as HTMLElement | null
    let ro: ResizeObserver | null = null
    if (nav) {
      ro = new ResizeObserver(getTop)
      ro.observe(nav)
    }
    return () => {
      window.removeEventListener('resize', onResize)
      ro?.disconnect()
    }
  }, [])
  return top
}

// Optimistic play/pause with auto-reconcile
function useOptimisticPlaying(realPlaying: boolean | undefined) {
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

// Button feedback
function usePressFeedback() {
  const press = { play: false, next: false, prev: false } as const

  const bump = (_key: keyof typeof press, _ms = 140) => {}
  const reset = () => {}
  return { press, bump, reset }
}

// Live media (throttled progress updates)
function useMediaState(allowInitialHydrate: boolean) {
  const [snap, setSnap] = useState<PersistedSnapshot | null>(null)
  const [livePlayMs, setLivePlayMs] = useState<number>(0)

  const lastTick = useRef<number>(performance.now())
  const lastUiUpdateRef = useRef<number>(0)
  const livePlayMsRef = useRef<number>(0)
  const hydratedOnceRef = useRef(false)

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const ev = (args[0] ?? {}) as UsbEvent
      if (ev?.type === 'unplugged') {
        hydratedOnceRef.current = false
        setSnap(null)
        setLivePlayMs(0)
        livePlayMsRef.current = 0
        return
      }
      const inc = payloadFromLiveEvent(ev)
      if (!inc) return
      setSnap((prev) => {
        const merged = mergePayload(prev?.payload, inc)
        let nextPlay = merged.media?.MediaSongPlayTime ?? 0
        if (inc.media?.MediaSongPlayTime === undefined) {
          const prevPlay = prev?.payload.media?.MediaSongPlayTime
          if (typeof prevPlay === 'number') nextPlay = prevPlay
        }
        setLivePlayMs(nextPlay)
        livePlayMsRef.current = nextPlay
        lastTick.current = performance.now()
        lastUiUpdateRef.current = lastTick.current
        return { timestamp: new Date().toISOString(), payload: merged }
      })
    }

    // Typed view of the pieces we use on window (no `any`)
    type Bridge = {
      carplay?: {
        ipc?: { onEvent?: (cb: (e: unknown, ...a: unknown[]) => void) => void | (() => void) }
      }
      electron?: {
        ipcRenderer?: {
          removeListener?: (channel: string, listener: (...a: unknown[]) => void) => void
        }
      }
    }
    const w = window as unknown as Bridge

    let unsubscribe: (() => void) | undefined
    if (typeof w.carplay?.ipc?.onEvent === 'function') {
      const maybe = w.carplay.ipc.onEvent(handler)
      if (typeof maybe === 'function') unsubscribe = maybe
    }

    return () => {
      if (typeof unsubscribe === 'function') {
        try {
          unsubscribe()
        } catch {}
        return
      }
      const remove = w.electron?.ipcRenderer?.removeListener
      if (typeof remove === 'function') {
        try {
          remove('carplay-event', handler as (...a: unknown[]) => void)
        } catch {}
      }
    }
  }, [])

  useEffect(() => {
    if (!allowInitialHydrate || hydratedOnceRef.current) return
    let cancelled = false
    ;(async () => {
      try {
        const initial = await window.carplay.ipc.readMedia()
        if (!cancelled && initial) {
          hydratedOnceRef.current = true
          setSnap(initial)
          const t0 = initial.payload.media?.MediaSongPlayTime ?? 0
          setLivePlayMs(t0)
          livePlayMsRef.current = t0
          lastTick.current = performance.now()
          lastUiUpdateRef.current = lastTick.current
        }
      } catch {}
    })()
    return () => {
      cancelled = true
    }
  }, [allowInitialHydrate])

  useEffect(() => {
    let raf = 0
    const UI_INTERVAL_MS = 120

    const loop = () => {
      raf = requestAnimationFrame(loop)
      const m = snap?.payload.media
      if (!m) return

      const now = performance.now()
      const dt = now - lastTick.current
      lastTick.current = now

      if (m.MediaPlayStatus === 1) {
        const dur = m.MediaSongDuration ?? 0
        const next = clamp((livePlayMsRef.current ?? 0) + dt, 0, dur)
        livePlayMsRef.current = next

        if (now - lastUiUpdateRef.current >= UI_INTERVAL_MS) {
          lastUiUpdateRef.current = now
          setLivePlayMs(next)
        }
      }
    }

    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [snap])

  return { snap, livePlayMs }
}

// Component
export const Media = () => {
  const isStreaming = useStatusStore((s) => s.isStreaming)

  const top = useBelowNavTop()
  const [rootRef, { w, h }] = useElementSize<HTMLDivElement>()
  const { snap, livePlayMs } = useMediaState(isStreaming)

  // Scales
  const minSide = Math.min(w, h)
  const titlePx = Math.round(clamp(minSide * 0.07, 22, 48))
  const artistPx = Math.round(clamp(minSide * 0.034, 14, 24))
  const albumPx = Math.round(clamp(minSide * 0.028, 13, 20))
  const pagePad = Math.round(clamp(minSide * 0.02, 12, 22))
  const colGap = Math.round(clamp(w * 0.025, 16, 28))
  const sectionGap = Math.round(clamp(h * 0.03, 10, 24))
  const ctrlSize = Math.round(clamp(h * 0.095, 50, 82))
  const ctrlGap = Math.round(clamp(w * 0.03, 16, 32))
  const progressH = Math.round(clamp(h * 0.012, 8, 12))

  // Layout + artwork
  const bottomDockH = ctrlSize + 16 + (progressH + 20)
  const contentH = Math.max(0, h - pagePad * 2 - bottomDockH)
  const innerW = Math.max(0, w - pagePad * 2)
  const MIN_TEXT_COL = 400
  const MIN_ART_COL = 140
  const canTwoCol = innerW >= MIN_TEXT_COL + MIN_ART_COL + colGap
  const textEst = titlePx * 1.25 + artistPx * 1.25 + albumPx * 1.1 + 40
  const artFromH = Math.max(130, contentH - Math.max(60, Math.min(textEst, contentH * 0.6)))
  const artWidthAllowance = Math.max(MIN_ART_COL, Math.floor(innerW - MIN_TEXT_COL - colGap))
  const artPx = canTwoCol
    ? Math.round(clamp(Math.min(contentH, artWidthAllowance), 140, 340))
    : Math.round(clamp(Math.min(h * 0.52, artFromH), 130, 320))

  // Media projection
  const m = snap?.payload.media
  const base64 = snap?.payload.base64Image
  const guessedMime = base64 && base64.startsWith('/9j/') ? 'image/jpeg' : 'image/png'
  const title = m?.MediaSongName ?? '—'
  const artist = m?.MediaArtistName ?? '—'
  const album = m?.MediaAlbumName ?? '—'
  const appName = m?.MediaAPPName ?? '—'
  const durationMs = m?.MediaSongDuration ?? 0
  const realPlaying = m?.MediaPlayStatus === 1
  const imageDataUrl = base64 ? `data:${guessedMime};base64,${base64}` : null

  const { uiPlaying, setOverride, clearOverride } = useOptimisticPlaying(realPlaying)
  const { press, bump, reset: resetPress } = usePressFeedback()

  // Per-button focus
  const [focus, setFocus] = useState<{ play: boolean; next: boolean; prev: boolean }>({
    play: false,
    next: false,
    prev: false
  })

  // Refs for visual flash
  const prevBtnRef = useRef<HTMLButtonElement | null>(null)
  const playBtnRef = useRef<HTMLButtonElement | null>(null)
  const nextBtnRef = useRef<HTMLButtonElement | null>(null)

  function flash(ref: React.RefObject<HTMLButtonElement | null>, ms = 140) {
    const el = ref.current
    if (!el) return
    const prevTransform = el.style.transform
    const prevShadow = el.style.boxShadow
    el.style.transform = 'scale(0.94)'
    el.style.boxShadow = '0 0 0 5px rgba(255,255,255,0.35) inset'
    window.setTimeout(() => {
      el.style.transform = prevTransform
      el.style.boxShadow = prevShadow
    }, ms)
  }

  // Backward-jump guard controls
  const prevElapsedRef = useRef(0)
  const allowBackwardOnceRef = useRef(false)

  // Commands
  const onPlayPause = () => {
    bump('play')
    flash(playBtnRef)
    const next = !uiPlaying
    setOverride(next)
    if (next) window.carplay.ipc.sendKeyCommand('play')
    else window.carplay.ipc.sendKeyCommand('pause')
  }

  const onPrev = () => {
    bump('prev')
    flash(prevBtnRef)
    allowBackwardOnceRef.current = true
    window.carplay.ipc.sendKeyCommand('prev')
  }
  const onNext = () => {
    bump('next')
    flash(nextBtnRef)
    window.carplay.ipc.sendKeyCommand('next')
  }

  useEffect(() => {
    const handler = (e: Event) => {
      const cmd = (e as CustomEvent<{ command?: string }>).detail?.command?.toLowerCase()
      if (!cmd) return
      if (cmd === 'play' || cmd === 'pause' || cmd === 'stop') {
        bump('play')
        flash(playBtnRef)
      } else if (cmd === 'next') {
        bump('next')
        flash(nextBtnRef)
      } else if (cmd === 'prev') {
        bump('prev')
        flash(prevBtnRef)
        allowBackwardOnceRef.current = true
      }
    }
    window.addEventListener('car-media-key', handler as EventListener)
    return () => window.removeEventListener('car-media-key', handler as EventListener)
  }, [bump])

  // Clear overrides on unplug
  useEffect(() => {
    const usbHandler = (_evt: unknown, ...args: unknown[]) => {
      const data = (args[0] ?? {}) as UsbEvent
      if (data?.type === 'unplugged') {
        clearOverride()
        resetPress()
      }
    }
    window.carplay.usb.listenForEvents(usbHandler)
    return () => window.carplay.usb.unlistenForEvents(usbHandler)
  }, [clearOverride, resetPress])

  // Progress from elapsed/total
  const elapsedMs = Math.max(0, livePlayMs || 0)
  const totalMs = Math.max(0, durationMs || 0)
  const lastProgressRef = useRef(0)
  const lastTrackSigRef = useRef<string>('')

  const trackSig = useMemo(
    () => [title, artist, album, totalMs].join('␟'),
    [title, artist, album, totalMs]
  )
  if (trackSig !== lastTrackSigRef.current) {
    lastTrackSigRef.current = trackSig
    lastProgressRef.current = 0
    prevElapsedRef.current = 0
  }

  const prevElapsed = prevElapsedRef.current
  const isRestart = allowBackwardOnceRef.current || prevElapsed - elapsedMs > 500

  let progress = totalMs > 0 ? elapsedMs / totalMs : 0

  // Block jitter while playing, but allow explicit restarts/back
  if (realPlaying && !isRestart && progress + 0.001 < lastProgressRef.current) {
    progress = lastProgressRef.current
  }

  progress = clamp(progress, 0, 1)
  lastProgressRef.current = progress
  prevElapsedRef.current = elapsedMs
  allowBackwardOnceRef.current = false

  const pct = Math.round(progress * 1000) / 10

  const iconPx = Math.round(ctrlSize * 0.46)
  const iconMainPx = Math.round(ctrlSize * 0.52)
  const textSidePad = Math.max(8, Math.round(pagePad * 0.75))

  return (
    <div
      id="media-root"
      ref={rootRef}
      style={{
        position: 'fixed',
        left: 0,
        right: 0,
        top,
        bottom: 0,
        zIndex: 1,
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        padding: pagePad,
        boxSizing: 'border-box'
      }}
    >
      {/* CONTENT */}
      <div style={{ flex: 1, minHeight: 0 }}>
        {canTwoCol ? (
          <div
            style={{
              height: '100%',
              display: 'grid',
              gridTemplateColumns: `minmax(${MIN_TEXT_COL}px, 1fr) ${artPx}px`,
              alignItems: 'center',
              columnGap: colGap,
              minHeight: 0
            }}
          >
            <div
              style={{
                height: '100%',
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'center',
                gap: sectionGap,
                minHeight: 0,
                paddingLeft: textSidePad
              }}
            >
              <div>
                <div
                  style={{
                    fontSize: `${titlePx}px`,
                    fontWeight: 800,
                    lineHeight: 1.08,
                    letterSpacing: 0.2
                  }}
                >
                  {title}
                </div>
                <div style={{ opacity: 0.9, fontSize: `${artistPx}px`, marginTop: 8 }}>
                  {artist}
                </div>
                <div style={{ opacity: 0.7, fontSize: `${albumPx}px`, marginTop: 4 }}>{album}</div>
                <div style={{ opacity: 0.55, fontSize: 12, marginTop: 4 }}>{appName}</div>
              </div>
            </div>

            <div
              style={{
                width: artPx,
                height: artPx,
                borderRadius: 18,
                overflow: 'hidden',
                background: 'rgba(255,255,255,0.06)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                marginLeft: 'auto'
              }}
            >
              {imageDataUrl ? (
                <img
                  src={imageDataUrl}
                  alt="Cover"
                  style={{ width: '100%', height: '100%', objectFit: 'cover' }}
                />
              ) : (
                <div style={{ opacity: 0.6, fontSize: 12 }}>No Artwork</div>
              )}
            </div>
          </div>
        ) : (
          <div
            style={{
              height: '100%',
              display: 'flex',
              flexDirection: 'column',
              justifyContent: 'center',
              gap: sectionGap,
              minHeight: 0,
              paddingLeft: textSidePad,
              paddingRight: textSidePad
            }}
          >
            <div>
              <div
                style={{
                  fontSize: `${titlePx}px`,
                  fontWeight: 800,
                  lineHeight: 1.08,
                  letterSpacing: 0.2
                }}
              >
                {title}
              </div>
              <div style={{ opacity: 0.9, fontSize: `${artistPx}px`, marginTop: 8 }}>{artist}</div>
              <div style={{ opacity: 0.7, fontSize: `${albumPx}px`, marginTop: 4 }}>{album}</div>
              <div style={{ opacity: 0.55, fontSize: 12, marginTop: 4 }}>{appName}</div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'center' }}>
              <div
                style={{
                  width: artPx,
                  height: artPx,
                  borderRadius: 18,
                  overflow: 'hidden',
                  background: 'rgba(255,255,255,0.06)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center'
                }}
              >
                {imageDataUrl ? (
                  <img
                    src={imageDataUrl}
                    alt="Cover"
                    style={{ width: '100%', height: '100%', objectFit: 'cover' }}
                  />
                ) : (
                  <div style={{ opacity: 0.6, fontSize: 12 }}>No Artwork</div>
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* BOTTOM DOCK */}
      <div style={{ display: 'grid', gridAutoRows: 'auto', rowGap: 10 }}>
        {/* Controls */}
        <div
          style={{ display: 'grid', gridTemplateColumns: '80px 1fr 80px', alignItems: 'center' }}
        >
          <div />
          <div style={{ justifySelf: 'center' }}>
            <div
              style={{
                display: 'flex',
                gap: ctrlGap,
                alignItems: 'center',
                height: Math.round(ctrlSize * 1.1)
              }}
            >
              <button
                ref={prevBtnRef}
                onMouseUp={(e) => (e.currentTarget as HTMLButtonElement).blur()}
                onFocus={() => setFocus((f) => ({ ...f, prev: true }))}
                onBlur={() => setFocus((f) => ({ ...f, prev: false }))}
                onClick={onPrev}
                title="Previous"
                aria-label="Previous"
                style={circleBtnStyle(ctrlSize, press.prev, focus.prev)}
              >
                <SkipPreviousIcon sx={{ fontSize: iconPx, display: 'block', lineHeight: 0 }} />
              </button>

              <button
                ref={playBtnRef}
                onMouseUp={(e) => (e.currentTarget as HTMLButtonElement).blur()}
                onFocus={() => setFocus((f) => ({ ...f, play: true }))}
                onBlur={() => setFocus((f) => ({ ...f, play: false }))}
                onClick={onPlayPause}
                title={uiPlaying ? 'Pause' : 'Play'}
                aria-label="Play/Pause"
                aria-pressed={uiPlaying}
                style={circleBtnStyle(Math.round(ctrlSize * 1.1), press.play, focus.play)}
              >
                {uiPlaying ? (
                  <PauseIcon sx={{ fontSize: iconMainPx, display: 'block', lineHeight: 0 }} />
                ) : (
                  <PlayArrowIcon
                    sx={{
                      fontSize: iconMainPx,
                      display: 'block',
                      lineHeight: 0,
                      transform: 'translateX(1px)'
                    }}
                  />
                )}
              </button>

              <button
                ref={nextBtnRef}
                onMouseUp={(e) => (e.currentTarget as HTMLButtonElement).blur()}
                onFocus={() => setFocus((f) => ({ ...f, next: true }))}
                onBlur={() => setFocus((f) => ({ ...f, next: false }))}
                onClick={onNext}
                title="Next"
                aria-label="Next"
                style={circleBtnStyle(ctrlSize, press.next, focus.next)}
              >
                <SkipNextIcon sx={{ fontSize: iconPx, display: 'block', lineHeight: 0 }} />
              </button>
            </div>
          </div>
          <div />
        </div>

        {/* Progress */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: '80px 1fr 80px',
            alignItems: 'center',
            columnGap: 12
          }}
        >
          <div style={{ fontSize: 14, opacity: 0.85 }}>{msToClock(elapsedMs)}</div>
          <div
            style={{
              height: progressH,
              borderRadius: progressH / 1.6,
              background: 'rgba(255,255,255,0.28)',
              overflow: 'hidden'
            }}
          >
            <div
              style={{
                width: `${pct}%`,
                height: '100%',
                transition: 'width 120ms linear',
                background: 'rgba(255,255,255,0.95)'
              }}
            />
          </div>
          <div style={{ fontSize: 14, opacity: 0.85, textAlign: 'right' }}>
            -{msToClock(Math.max(0, totalMs - elapsedMs))}
          </div>
        </div>
      </div>
    </div>
  )

  function circleBtnStyle(size: number, pressed = false, focused = false): React.CSSProperties {
    return {
      position: 'relative',
      width: size,
      height: size,
      borderRadius: '50%',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      border: 'none',
      background: 'rgba(255,255,255,0.18)',
      cursor: 'pointer',
      userSelect: 'none',
      WebkitTapHighlightColor: 'transparent',
      lineHeight: 0,
      outline: 'none',
      transform: pressed ? 'scale(0.94)' : 'scale(1)',
      transition: 'transform 110ms ease, box-shadow 110ms ease, background 110ms ease',
      boxShadow: focused
        ? '0 0 0 3px rgba(255,255,255,0.55)'
        : pressed
          ? '0 0 0 5px rgba(255,255,255,0.35) inset'
          : 'none'
    }
  }
}
