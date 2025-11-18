// Live media (throttled progress updates)

import { useEffect, useRef, useState } from 'react'
import { Bridge, PersistedSnapshot, UsbEvent } from '../types'
import { clamp, mergePayload, payloadFromLiveEvent } from '../utils'
import { UI_INTERVAL_MS } from '../constants'

export function useMediaState(allowInitialHydrate: boolean) {
  const [snap, setSnap] = useState<PersistedSnapshot | null>(null)
  const [livePlayMs, setLivePlayMs] = useState<number>(0)

  const lastTick = useRef<number>(performance.now())
  const lastUiUpdateRef = useRef<number>(0)
  const livePlayMsRef = useRef<number>(0)
  const hydratedOnceRef = useRef(false)

  useEffect(() => {
    const handler = (_evt: unknown, ...args: unknown[]) => {
      const ev = (args[0] ?? {}) as UsbEvent
      if (ev?.type === 'unplugged') {
        hydratedOnceRef.current = false
        setSnap(null)
        setLivePlayMs(0)
        livePlayMsRef.current = 0
        return
      }
      const inc = payloadFromLiveEvent(ev)
      if (!inc) return
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
    if (typeof w.carplay?.ipc?.onEvent === 'function') {
      const maybe = w.carplay.ipc.onEvent(handler)
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
          remove('carplay-event', handler as (...a: unknown[]) => void)
        } catch {}
      }
    }
  }, [])

  useEffect(() => {
    if (!allowInitialHydrate || hydratedOnceRef.current) return
    let cancelled = false
    ;(async () => {
      try {
        // eslint-disable-next-line @typescript-eslint/ban-ts-comment
        // @ts-ignore
        const initial = await window.carplay.ipc.readMedia()
        if (!cancelled && initial) {
          hydratedOnceRef.current = true
          setSnap(initial)
          const t0 = initial.payload.media?.MediaSongPlayTime ?? 0
          setLivePlayMs(t0)
          livePlayMsRef.current = t0
          lastTick.current = performance.now()
          lastUiUpdateRef.current = lastTick.current
        }
      } catch {}
    })()
    return () => {
      cancelled = true
    }
  }, [allowInitialHydrate])

  useEffect(() => {
    let raf = 0

    const loop = () => {
      raf = requestAnimationFrame(loop)
      const m = snap?.payload.media
      if (!m) return

      const now = performance.now()
      const dt = now - lastTick.current
      lastTick.current = now

      if (m.MediaPlayStatus === 1) {
        const dur = m.MediaSongDuration ?? 0
        const next = clamp((livePlayMsRef.current ?? 0) + dt, 0, dur)
        livePlayMsRef.current = next

        if (now - lastUiUpdateRef.current >= UI_INTERVAL_MS) {
          lastUiUpdateRef.current = now
          setLivePlayMs(next)
        }
      }
    }

    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [snap])

  return { snap, livePlayMs }
}
