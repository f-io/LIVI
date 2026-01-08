import { circleBtnStyle } from '../styles'
import PlayArrowIcon from '@mui/icons-material/PlayArrow'
import PauseIcon from '@mui/icons-material/Pause'
import SkipNextIcon from '@mui/icons-material/SkipNext'
import SkipPreviousIcon from '@mui/icons-material/SkipPrevious'
import { RefObject, SetStateAction } from 'react'

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
  )
}
