// Button feedback
// export function usePressFeedback() {
//   const press = { play: false, next: false, prev: false } as const
//
//   const bump = (_key: keyof typeof press, _ms = 140) => {}
//   const reset = () => {}
//   return { press, bump, reset }
// }

import { useCallback, useRef, useState } from 'react'

export function usePressFeedback() {
  const [press, setPress] = useState({
    play: false,
    next: false,
    prev: false
  })

  const timers = useRef<Record<keyof typeof press, number | null>>({
    play: null,
    next: null,
    prev: null
  })

  const bump = useCallback((key: keyof typeof press, ms = 140) => {
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
    setPress({ play: false, next: false, prev: false })
  }, [])

  return { press, bump, reset }
}
