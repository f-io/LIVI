import { flash } from './flash'
import { MediaEventType } from '../types'
import { RefObject } from 'react'

export const mediaControlOps = ({
  uiPlaying,
  onBump,
  playBtnRef,
  prevBtnRef,
  allowBackwardOnceRef,
  nextBtnRef,
  setOverride
}: {
  uiPlaying: boolean
  onBump: (type: MediaEventType) => void
  playBtnRef: RefObject<HTMLButtonElement | null>
  prevBtnRef: RefObject<HTMLButtonElement | null>
  allowBackwardOnceRef: RefObject<boolean>
  nextBtnRef: RefObject<HTMLButtonElement | null>
  setOverride: (type: boolean) => void
}) => {
  const handlePlayPause = () => {
    onBump(MediaEventType.PLAY)
    flash(playBtnRef)
    setOverride(!uiPlaying)

    window.carplay.ipc.sendKeyCommand(!uiPlaying ? MediaEventType.PLAY : MediaEventType.PAUSE)
  }

  const handlePrev = () => {
    onBump(MediaEventType.PREV)
    flash(prevBtnRef)
    allowBackwardOnceRef.current = true
    window.carplay.ipc.sendKeyCommand(MediaEventType.PREV)
  }
  const handleNext = () => {
    onBump(MediaEventType.NEXT)
    flash(nextBtnRef)
    window.carplay.ipc.sendKeyCommand(MediaEventType.NEXT)
  }

  return {
    onPlayPause: handlePlayPause,
    onPrev: handlePrev,
    onNext: handleNext
  }
}
