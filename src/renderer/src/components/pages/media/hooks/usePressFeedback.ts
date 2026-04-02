// Button feedback
import { useCallback, useRef, useState } from 'react'
import { FLASH_TIMEOUT_MS } from '../constants'
import { MediaEventType } from '../types'

export function usePressFeedback() {
  const [press, setPress] = useState<Record<MediaEventType, boolean>>({
    [MediaEventType.PLAY]: false,
    [MediaEventType.PAUSE]: false,
    [MediaEventType.PLAYPAUSE]: false,
    [MediaEventType.STOP]: false,
    [MediaEventType.PREV]: false,
    [MediaEventType.NEXT]: false
  })

  const timers = useRef<Record<keyof typeof press, number | null>>({
    play: null,
    pause: null,
    playpause: null,
    stop: null,
    next: null,
    prev: null
  })

  const bump = useCallback((key: keyof typeof press, ms = FLASH_TIMEOUT_MS) => {
    setPress((prev) => ({ ...prev, [key]: true }))

    if (timers.current[key]) window.clearTimeout(timers.current[key]!)
    timers.current[key] = window.setTimeout(() => {
      setPress((prev) => ({ ...prev, [key]: false }))
    }, ms)
  }, [])

  const reset = useCallback(() => {
    Object.keys(timers.current).forEach((key) => {
      const k = key as keyof typeof press
      if (timers.current[k]) window.clearTimeout(timers.current[k]!)
    })
    setPress({ play: false, pause: false, playpause: false, stop: false, next: false, prev: false })
  }, [])

  return { press, bump, reset }
}
