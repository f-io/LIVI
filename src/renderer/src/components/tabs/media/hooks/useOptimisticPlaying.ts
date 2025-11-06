// Optimistic play/pause with auto-reconcile
import { useEffect, useRef, useState } from 'react'

// deprecated
export function useOptimisticPlaying_deprecated(realPlaying: boolean | undefined) {
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

type Options = { timeoutMs?: number }

export function useOptimisticPlaying(
  realPlaying: boolean | undefined,
  mediaPayloadError: unknown,
  opts: Options = {}
) {
  const { timeoutMs } = opts
  const [override, setOverride] = useState<boolean | null>(null)

  // mark that override was set by user interaction
  const manualRef = useRef(false)
  const timeoutRef = useRef<number | null>(null)

  // helper to clear any running timer
  const clearTimer = () => {
    if (timeoutRef.current != null) {
      window.clearTimeout(timeoutRef.current)
      timeoutRef.current = null
    }
  }

  // function the component should call to set user-driven override
  const setOverrideByUser = (v: boolean) => {
    manualRef.current = true
    setOverride(v)

    // if user provided a timeout and there is no payload error, start timer to auto-clear
    clearTimer()
    if (typeof timeoutMs === 'number' && !mediaPayloadError) {
      timeoutRef.current = window.setTimeout(() => {
        manualRef.current = false
        setOverride(null)
        timeoutRef.current = null
      }, timeoutMs)
    }
  }

  const clearOverride = () => {
    manualRef.current = false
    clearTimer()
    setOverride(null)
  }

  // react to incoming realPlaying / mediaPayloadError updates
  useEffect(() => {
    const isError = Boolean(mediaPayloadError)

    // If we have a manual override and there is an active error, ignore incoming realPlaying
    if (manualRef.current && isError) {
      // do nothing: keep UI as user set it
      return
    }

    // If override exists and realPlaying matches it, and there's no error, then clear manual override
    if (
      override !== null &&
      typeof realPlaying === 'boolean' &&
      realPlaying === override &&
      !isError
    ) {
      manualRef.current = false
      clearTimer()
      setOverride(null)
      return
    }

    // If there is no manual override, always reflect realPlaying
    if (!manualRef.current) {
      // if realPlaying is boolean, keep override null and rely on realPlaying
      // (uiPlaying will use realPlaying when override is null)
      clearTimer()
      // do not call setOverride(null) unnecessarily (avoid rerenders) — only if override is not null
      if (override !== null) setOverride(null)
    }
    // otherwise (manualRef true but no error): keep manual override until realPlaying confirms or timeout
  }, [realPlaying, mediaPayloadError, override])

  useEffect(() => {
    return () => {
      clearTimer()
    }
  }, [])

  const uiPlaying = override ?? !!realPlaying

  return {
    uiPlaying,
    setOverride: setOverrideByUser,
    clearOverride,
    // for tests/inspections
    _internal: {
      manualRef
    }
  }
}
