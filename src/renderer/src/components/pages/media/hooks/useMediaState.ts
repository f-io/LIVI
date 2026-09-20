// Live media (throttled progress updates)

import { useCallback, useEffect, useRef, useState } from 'react'
import { PROGRESS_STALL_MS, UI_INTERVAL_MS } from '../constants'
import { Bridge, PersistedSnapshot, UsbEvent } from '../types'
import { clamp, mergePayload, payloadFromLiveEvent } from '../utils'

export function useMediaState(allowInitialHydrate: boolean) {
  const [snap, setSnap] = useState<PersistedSnapshot | null>(null)
  const [livePlayMs, setLivePlayMs] = useState<number>(0)
  const [stalled, setStalled] = useState(false)

  const lastTick = useRef<number>(performance.now())
  const lastUiUpdateRef = useRef<number>(0)
  const livePlayMsRef = useRef<number>(0)
  const hydratedOnceRef = useRef(false)
  const phonePlayMsRef = useRef<number | undefined>(undefined)
  const playStatusRef = useRef<number | undefined>(undefined)
  const lastProgressAtRef = useRef<number>(performance.now())

  const seedPlayTime = useCallback((from: PersistedSnapshot) => {
    const t0 = from.payload.media?.MediaSongPlayTime ?? 0
    setLivePlayMs(t0)
    livePlayMsRef.current = t0
    lastTick.current = performance.now()
    lastUiUpdateRef.current = lastTick.current
    phonePlayMsRef.current = t0
    playStatusRef.current = from.payload.media?.MediaPlayStatus
    lastProgressAtRef.current = lastTick.current
    setStalled(false)
  }, [])

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const ev = (args[0] ?? {}) as UsbEvent
      if (ev?.type === 'media-reset') {
        void (async () => {
          try {
            const next = await window.projection.ipc.readMedia()
            if (next) {
              setSnap(next)
              seedPlayTime(next)
            }
          } catch {}
        })()
        return
      }
      const inc = payloadFromLiveEvent(ev)
      if (!inc) return
      const phonePlayMs = inc.media?.MediaSongPlayTime
      const progressed = typeof phonePlayMs === 'number' && phonePlayMs !== phonePlayMsRef.current
      if (progressed) phonePlayMsRef.current = phonePlayMs
      const status = inc.media?.MediaPlayStatus
      const resumed = status === 1 && playStatusRef.current !== 1
      if (status !== undefined) playStatusRef.current = status
      if (progressed || resumed) {
        lastProgressAtRef.current = performance.now()
        setStalled(false)
      }
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

    const w = window as unknown as Bridge

    let unsubscribe: (() => void) | undefined
    if (typeof w.projection?.ipc?.onEvent === 'function') {
      const maybe = w.projection.ipc.onEvent(handler)
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
          remove('projection-event', handler as (...a: unknown[]) => void)
        } catch {}
      }
    }
  }, [seedPlayTime])

  useEffect(() => {
    if (!allowInitialHydrate || hydratedOnceRef.current) return
    let cancelled = false
    ;(async () => {
      try {
        // eslint-disable-next-line @typescript-eslint/ban-ts-comment
        const initial = await window.projection.ipc.readMedia()
        if (!cancelled && initial) {
          hydratedOnceRef.current = true
          setSnap(initial)
          seedPlayTime(initial)
        }
      } catch {}
    })()
    return () => {
      cancelled = true
    }
  }, [allowInitialHydrate, seedPlayTime])

  useEffect(() => {
    let raf = 0

    const loop = () => {
      raf = requestAnimationFrame(loop)
      const m = snap?.payload.media
      if (!m) return

      const now = performance.now()
      const dt = now - lastTick.current
      lastTick.current = now

      if (m.MediaPlayStatus !== 1) return

      if (now - lastProgressAtRef.current > PROGRESS_STALL_MS) {
        const reported = phonePlayMsRef.current
        if (typeof reported === 'number' && reported !== livePlayMsRef.current) {
          livePlayMsRef.current = reported
          setLivePlayMs(reported)
        }
        setStalled(true)
        return
      }

      const dur = m.MediaSongDuration ?? 0
      const next = clamp(livePlayMsRef.current + dt, 0, dur)
      livePlayMsRef.current = next

      if (now - lastUiUpdateRef.current >= UI_INTERVAL_MS) {
        lastUiUpdateRef.current = now
        setLivePlayMs(next)
      }
    }

    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [snap])

  return { snap, livePlayMs, stalled }
}
