import { useCallback, useEffect, useMemo, useRef, useState, Suspense, lazy } from 'react'
import { useStatusStore } from '@store/store'
import { MediaEventType, UsbEvent } from './types'
import {
  // useBelowNavTop,
  useElementSize,
  useMediaState,
  useOptimisticPlaying,
  usePressFeedback
} from './hooks'
import { clamp } from './utils'
import { ProgressBar, Controls } from './components'
import { EXTRA_SMALL_SCREEN, MIN_SCREEN_SIZE_FOR_ATRWORK, MIN_TEXT_COL } from './constants'
import { flash } from './utils/flash'
import { mediaScaleOps } from './utils/mediaScaleOps'
import { mediaLayoutArtworksOps } from './utils/mediaLayoutArtworksOps'
import { mediaProjectionOps } from './utils/mediaProjectionOps'
import { mediaControlOps } from './utils/mediaControllOps'

const FFTSpectrum = lazy(() =>
  import('./components/FFTSpectrum').then((m) => ({ default: m.FFTSpectrum }))
)

export const Media = () => {
  const isStreaming = useStatusStore((s: { isStreaming: boolean }) => s.isStreaming)

  // const top = useBelowNavTop()
  const [rootRef, { w, h }] = useElementSize<HTMLDivElement>()
  const { snap, livePlayMs } = useMediaState(isStreaming)

  // Scales (base)
  const { titlePx, artistPx, albumPx, pagePad, colGap, sectionGap, ctrlSize, ctrlGap, progressH } =
    mediaScaleOps({ w, h })

  // Tiny-screen helpers (e.g. 320x240)
  const isTinyHeight = h > 0 && h <= 320
  const textScale = isTinyHeight ? 0.88 : 1

  // Clamp padding on tiny screens, otherwise the progress bar has no space left
  const pagePadClamped = isTinyHeight ? Math.min(pagePad, 10) : pagePad

  const titlePxScaled = Math.max(12, Math.round(titlePx * textScale))
  const artistPxScaled = Math.max(11, Math.round(artistPx * textScale))
  const albumPxScaled = Math.max(10, Math.round(albumPx * textScale))
  const appPxScaled = Math.max(10, Math.round(12 * textScale))

  // Slightly slimmer bar on tiny screens
  const progressHScaled = Math.max(6, Math.round(progressH * (isTinyHeight ? 0.85 : 1)))

  // Compute usable inner width (hard clamp) for anything that must not overflow
  const innerMaxWidth = Math.max(0, Math.floor(w - pagePadClamped * 2))

  // Layout + artwork
  const { canTwoCol, artPx, innerW } = mediaLayoutArtworksOps({
    ctrlSize,
    progressH: progressHScaled,
    w,
    h,
    pagePad: pagePadClamped,
    colGap,
    titlePx: titlePxScaled,
    artistPx: artistPxScaled,
    albumPx: albumPxScaled
  })

  // Media projection
  const {
    mediaPayloadError,
    title,
    artist,
    album,
    appName,
    durationMs,
    realPlaying,
    imageDataUrl
  } = mediaProjectionOps({ snap })

  const { uiPlaying, setOverride, clearOverride } = useOptimisticPlaying(
    realPlaying,
    mediaPayloadError
  )
  const { press, bump, reset: resetPress } = usePressFeedback()

  // Artwork <-> FFT toggle
  const [showFft, setShowFft] = useState(false)

  const toggleArtworkFft = useCallback(() => {
    setShowFft((v) => !v)
  }, [])

  // Enable visualizer only when FFT is visible (and streaming)
  useEffect(() => {
    const enabled = !!showFft && !!isStreaming
    window.carplay?.ipc?.setVisualizerEnabled?.(enabled)
    return () => window.carplay?.ipc?.setVisualizerEnabled?.(false)
  }, [showFft, isStreaming])

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

  // Backward-jump guard controls
  const prevElapsedRef = useRef(0)
  const allowBackwardOnceRef = useRef(false)

  const { onPlayPause, onPrev, onNext } = mediaControlOps({
    uiPlaying,
    onBump: bump,
    playBtnRef,
    prevBtnRef,
    allowBackwardOnceRef,
    nextBtnRef,
    setOverride
  })

  useEffect(() => {
    const handler = (e: Event) => {
      const cmd = (e as CustomEvent<{ command?: string }>).detail?.command?.toLowerCase()
      if (!cmd) return

      if (
        cmd === MediaEventType.PLAY ||
        cmd === MediaEventType.PAUSE ||
        cmd === MediaEventType.STOP
      ) {
        bump(MediaEventType.PLAY)
        flash(playBtnRef)
      } else if (cmd === MediaEventType.NEXT) {
        bump(MediaEventType.NEXT)
        flash(nextBtnRef)
      } else if (cmd === MediaEventType.PREV) {
        bump(MediaEventType.PREV)
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
        setShowFft(false)
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

  // Slightly reduce side padding for text on tiny screens
  const textSidePad = Math.max(6, Math.round(pagePadClamped * 0.75))

  // Always show progress bar (unless payload error)
  const showProgressBar = !mediaPayloadError

  const ART_ROUND = canTwoCol ? 34 : 18
  const fftPad = Math.max(8, Math.round(artPx * 0.06))

  // IMPORTANT:
  // - We keep rounded clipping ONLY for artwork.
  // - For FFT we do NOT round/clip; we also slightly scaleY to avoid the "stretched" look.
  const artworkBoxStyle: React.CSSProperties = {
    width: artPx,
    height: artPx,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    cursor: 'pointer',
    userSelect: 'none'
  }

  const onArtworkKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      e.stopPropagation()
      toggleArtworkFft()
    }
  }

  const ArtworkOrFft = (
    <div
      role="button"
      tabIndex={0}
      onClick={toggleArtworkFft}
      onKeyDown={onArtworkKeyDown}
      style={artworkBoxStyle}
      aria-label={showFft ? 'Show artwork' : 'Show spectrum'}
      title={showFft ? 'Show artwork' : 'Show spectrum'}
    >
      {showFft ? (
        <div
          className="fft-surface"
          style={{
            width: '100%',
            height: '100%',
            borderRadius: 0,
            overflow: 'visible',
            padding: fftPad,
            boxSizing: 'border-box',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center'
          }}
        >
          <div
            className="fft-surface-inner"
            style={{
              width: '100%',
              height: '100%',
              transform: 'scaleY(0.85)',
              transformOrigin: 'center',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center'
            }}
          >
            <Suspense fallback={null}>
              <FFTSpectrum />
            </Suspense>
          </div>
        </div>
      ) : (
        <div
          className="artwork-surface"
          style={{
            width: '100%',
            height: '100%',
            borderRadius: ART_ROUND,
            overflow: 'hidden',
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
              draggable={false}
            />
          ) : (
            <div style={{ opacity: 0.6, fontSize: 12 }}>No Artwork</div>
          )}
        </div>
      )}
    </div>
  )

  return (
    <div
      id="media-root"
      ref={rootRef}
      style={{
        position: 'fixed',
        left: '75px',
        right: 0,
        top: 0,
        bottom: 0,
        zIndex: 1,
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        padding: pagePadClamped,
        boxSizing: 'border-box',
        overflow: 'hidden'
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
              <div style={{ maxWidth: innerMaxWidth, overflow: 'hidden' }}>
                <div
                  style={{
                    fontSize: `${titlePxScaled}px`,
                    fontWeight: 800,
                    lineHeight: 1.08,
                    letterSpacing: 0.2,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis'
                  }}
                >
                  {title}
                </div>
                <div
                  style={{
                    opacity: 0.9,
                    fontSize: `${artistPxScaled}px`,
                    marginTop: 8,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis'
                  }}
                >
                  {artist}
                </div>
                <div
                  style={{
                    opacity: 0.7,
                    fontSize: `${albumPxScaled}px`,
                    marginTop: 4,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis'
                  }}
                >
                  {album}
                </div>
                <div style={{ opacity: 0.55, fontSize: appPxScaled, marginTop: 4 }}>{appName}</div>
              </div>
            </div>

            <div style={{ marginLeft: 'auto' }}>{ArtworkOrFft}</div>
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
            <div style={{ maxWidth: innerMaxWidth, overflow: 'hidden' }}>
              <div
                style={{
                  fontSize: `${titlePxScaled}px`,
                  fontWeight: 800,
                  lineHeight: 1.08,
                  letterSpacing: 0.2,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis'
                }}
              >
                {title}
              </div>

              {innerW > EXTRA_SMALL_SCREEN && (
                <>
                  <div
                    style={{
                      opacity: 0.9,
                      fontSize: `${artistPxScaled}px`,
                      marginTop: 8,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis'
                    }}
                  >
                    {artist}
                  </div>
                  <div
                    style={{
                      opacity: 0.7,
                      fontSize: `${albumPxScaled}px`,
                      marginTop: 4,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis'
                    }}
                  >
                    {album}
                  </div>
                </>
              )}

              <div style={{ opacity: 0.55, fontSize: appPxScaled, marginTop: 4 }}>
                {innerW > EXTRA_SMALL_SCREEN || !artist ? appName : artist}
              </div>
            </div>

            {innerW > MIN_SCREEN_SIZE_FOR_ATRWORK && (
              <div style={{ display: 'flex', justifyContent: 'center' }}>{ArtworkOrFft}</div>
            )}
          </div>
        )}
      </div>

      {/* BOTTOM DOCK */}
      <div
        style={{
          display: 'grid',
          gridAutoRows: 'auto',
          rowGap: isTinyHeight ? 8 : 10,
          paddingBottom: isTinyHeight ? 6 : '1rem',
          width: '100%',
          boxSizing: 'border-box'
        }}
      >
        {/* Always center controls */}
        <div style={{ display: 'flex', justifyContent: 'center', width: '100%' }}>
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
        </div>

        {/* Always render progress bar (unless payload error) */}
        {showProgressBar && (
          <div style={{ width: '100%', maxWidth: '100%', boxSizing: 'border-box' }}>
            <ProgressBar
              elapsedMs={elapsedMs}
              progressH={progressHScaled}
              totalMs={totalMs}
              pct={pct}
            />
          </div>
        )}
      </div>
    </div>
  )
}
