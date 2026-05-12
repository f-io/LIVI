import { useEffect, useState } from 'react'

export type TransportState = {
  active: 'dongle' | 'aa' | null
  dongleDetected: boolean
  nativeDetected: boolean
  preference: 'auto' | 'dongle' | 'native'
}

const INITIAL: TransportState = {
  active: null,
  dongleDetected: false,
  nativeDetected: false,
  preference: 'auto'
}

export function useTransportState(): TransportState {
  const [state, setState] = useState<TransportState>(INITIAL)

  useEffect(() => {
    let cancelled = false

    const api = (window as { projection?: any }).projection
    if (!api?.ipc?.getTransportState || !api?.ipc?.onEvent) return

    api.ipc
      .getTransportState()
      .then((s: TransportState) => {
        if (!cancelled && s) setState(s)
      })
      .catch(() => {})

    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = args[0] as { type?: string; payload?: TransportState } | undefined
      if (msg?.type !== 'transportState' || !msg.payload) return
      setState(msg.payload)
    }
    api.ipc.onEvent(handler)

    return () => {
      cancelled = true
      api.ipc.offEvent?.(handler)
    }
  }, [])

  return state
}
