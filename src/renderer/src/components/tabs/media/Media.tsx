import { useEffect, useMemo, useRef, useState } from 'react'
import { useStatusStore } from '@store/store'
import { UsbEvent } from './types'
import {
  useBelowNavTop,
  useElementSize,
  useMediaState,
  useOptimisticPlaying,
  usePressFeedback
} from './hooks'
import { clamp } from './utils'
import { ProgressBar, Controls } from './components'

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
    if (next) {
      // eslint-disable-next-line @typescript-eslint/ban-ts-comment
      // @ts-ignore
      window.carplay.ipc.sendKeyCommand('play')
    }
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    else window.carplay.ipc.sendKeyCommand('pause')
  }

  const onPrev = () => {
    bump('prev')
    flash(prevBtnRef)
    allowBackwardOnceRef.current = true
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    window.carplay.ipc.sendKeyCommand('prev')
  }
  const onNext = () => {
    bump('next')
    flash(nextBtnRef)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
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
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
    window.carplay.usb.listenForEvents(usbHandler)
    // eslint-disable-next-line @typescript-eslint/ban-ts-comment
    // @ts-ignore
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
        <Controls
          ctrlGap={ctrlGap}
          ctrlSize={ctrlSize}
          prevBtnRef={prevBtnRef}
          playBtnRef={playBtnRef}
          nextBtnRef={nextBtnRef}
          onSetFocus={setFocus}
          onPrev={onPrev}
          onPlayPause={onPlayPause}
          onNext={onNext}
          uiPlaying={uiPlaying}
          press={press}
          focus={focus}
          iconPx={iconPx}
          iconMainPx={iconMainPx}
        />

        <ProgressBar elapsedMs={elapsedMs} progressH={progressH} totalMs={totalMs} pct={pct} />
      </div>
    </div>
  )
}
