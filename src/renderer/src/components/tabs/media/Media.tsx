import { useEffect, useMemo, useRef, useState } from 'react'
import { useStatusStore } from '@store/store'
import { MediaEventType, UsbEvent } from './types'
import {
  useBelowNavTop,
  useElementSize,
  useMediaState,
  useOptimisticPlaying,
  usePressFeedback
} from './hooks'
import { clamp } from './utils'
import { ProgressBar, Controls } from './components'
import {
  EXTRA_SMALL_SCREEN,
  MIN_SCREEN_SIZE_FOR_ATRWORK,
  MIN_SCREEN_SIZE_FOR_PROGRESSBAR,
  MIN_TEXT_COL
} from './constants'
import { flash } from './utils/flash'
import { mediaScaleOps } from './utils/mediaScaleOps'
import { mediaLayoutArtworksOps } from './utils/mediaLayoutArtworksOps'
import { mediaProjectionOps } from './utils/mediaProjectionOps'
import { mediaControlOps } from './utils/mediaControllOps'

export const Media = () => {
  const isStreaming = useStatusStore((s: { isStreaming: boolean }) => s.isStreaming)

  const top = useBelowNavTop()
  const [rootRef, { w, h }] = useElementSize<HTMLDivElement>()
  const { snap, livePlayMs } = useMediaState(isStreaming)

  // Scales
  const { titlePx, artistPx, albumPx, pagePad, colGap, sectionGap, ctrlSize, ctrlGap, progressH } =
    mediaScaleOps({ w, h })

  // Layout + artwork
  const { canTwoCol, artPx, innerW } = mediaLayoutArtworksOps({
    ctrlSize,
    progressH,
    w,
    h,
    pagePad,
    colGap,
    titlePx,
    artistPx,
    albumPx
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
                borderRadius: 34,
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
              {innerW > EXTRA_SMALL_SCREEN && (
                <>
                  <div style={{ opacity: 0.9, fontSize: `${artistPx}px`, marginTop: 8 }}>
                    {artist}
                  </div>
                  <div style={{ opacity: 0.7, fontSize: `${albumPx}px`, marginTop: 4 }}>
                    {album}
                  </div>
                </>
              )}

              <div style={{ opacity: 0.55, fontSize: 12, marginTop: 4 }}>{appName}</div>
            </div>

            {innerW > MIN_SCREEN_SIZE_FOR_ATRWORK && (
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
            )}
          </div>
        )}
      </div>

      {/* BOTTOM DOCK */}
      <div style={{ display: 'grid', gridAutoRows: 'auto', rowGap: 10, paddingBottom: '1rem' }}>
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

        {!mediaPayloadError && innerW > MIN_SCREEN_SIZE_FOR_PROGRESSBAR && (
          <ProgressBar elapsedMs={elapsedMs} progressH={progressH} totalMs={totalMs} pct={pct} />
        )}
      </div>
    </div>
  )
}
