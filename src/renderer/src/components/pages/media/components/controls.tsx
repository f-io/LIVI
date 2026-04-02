import PauseIcon from '@mui/icons-material/Pause'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import SkipNextIcon from '@mui/icons-material/SkipNext'
import SkipPreviousIcon from '@mui/icons-material/SkipPrevious'
import { useTheme } from '@mui/material/styles'
import { RefObject, SetStateAction, useState } from 'react'
import { circleBtnStyle } from '../styles'

type ControlsProps = {
  ctrlGap: number
  ctrlSize: number
  prevBtnRef: RefObject<HTMLButtonElement | null>
  playBtnRef: RefObject<HTMLButtonElement | null>
  nextBtnRef: RefObject<HTMLButtonElement | null>
  onSetFocus: (
    focus: SetStateAction<{
      play: boolean
      next: boolean
      prev: boolean
    }>
  ) => void
  onPrev: () => void
  onPlayPause: () => void
  onNext: () => void
  uiPlaying: boolean
  press: {
    play?: boolean
    next?: boolean
    prev?: boolean
  }
  focus: {
    play?: boolean
    next?: boolean
    prev?: boolean
  }
  iconPx: number
  iconMainPx: number
}

export const Controls = ({
  ctrlGap,
  ctrlSize,
  prevBtnRef,
  playBtnRef,
  nextBtnRef,
  onSetFocus: setFocus,
  onPrev,
  onPlayPause,
  onNext,
  uiPlaying,
  press,
  focus,
  iconPx,
  iconMainPx
}: ControlsProps) => {
  const theme = useTheme()
  const ringColor = theme.palette.primary.main

  const [hover, setHover] = useState<{ play: boolean; next: boolean; prev: boolean }>({
    play: false,
    next: false,
    prev: false
  })

  return (
    <div
      style={{
        width: '100%',
        display: 'flex',
        justifyContent: 'center'
      }}
    >
      <div
        style={{
          display: 'flex',
          gap: ctrlGap,
          alignItems: 'center',
          height: Math.round(ctrlSize * 1.1)
        }}
      >
        {/* PREVIOUS */}
        <button
          ref={prevBtnRef}
          onMouseUp={(e) => (e.currentTarget as HTMLButtonElement).blur()}
          onFocus={() => setFocus((f) => ({ ...f, prev: true }))}
          onBlur={() => setFocus((f) => ({ ...f, prev: false }))}
          onMouseEnter={() => setHover((h) => ({ ...h, prev: true }))}
          onMouseLeave={() => setHover((h) => ({ ...h, prev: false }))}
          onClick={onPrev}
          title="Previous"
          aria-label="Previous"
          style={circleBtnStyle(ctrlSize, {
            pressed: !!press.prev,
            focused: !!focus.prev,
            hovered: hover.prev,
            ringColor
          })}
        >
          <SkipPreviousIcon sx={{ fontSize: iconPx, display: 'block', lineHeight: 0 }} />
        </button>

        {/* PLAY / PAUSE */}
        <button
          ref={playBtnRef}
          onMouseUp={(e) => (e.currentTarget as HTMLButtonElement).blur()}
          onFocus={() => setFocus((f) => ({ ...f, play: true }))}
          onBlur={() => setFocus((f) => ({ ...f, play: false }))}
          onMouseEnter={() => setHover((h) => ({ ...h, play: true }))}
          onMouseLeave={() => setHover((h) => ({ ...h, play: false }))}
          onClick={onPlayPause}
          title={uiPlaying ? 'Pause' : 'Play'}
          aria-label="Play/Pause"
          aria-pressed={uiPlaying}
          style={circleBtnStyle(Math.round(ctrlSize * 1.1), {
            pressed: !!press.play,
            focused: !!focus.play,
            hovered: hover.play,
            ringColor
          })}
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

        {/* NEXT */}
        <button
          ref={nextBtnRef}
          onMouseUp={(e) => (e.currentTarget as HTMLButtonElement).blur()}
          onFocus={() => setFocus((f) => ({ ...f, next: true }))}
          onBlur={() => setFocus((f) => ({ ...f, next: false }))}
          onMouseEnter={() => setHover((h) => ({ ...h, next: true }))}
          onMouseLeave={() => setHover((h) => ({ ...h, next: false }))}
          onClick={onNext}
          title="Next"
          aria-label="Next"
          style={circleBtnStyle(ctrlSize, {
            pressed: !!press.next,
            focused: !!focus.next,
            hovered: hover.next,
            ringColor
          })}
        >
          <SkipNextIcon sx={{ fontSize: iconPx, display: 'block', lineHeight: 0 }} />
        </button>
      </div>
    </div>
  )
}
