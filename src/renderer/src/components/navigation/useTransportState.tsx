import type { TransportSnapshot } from '@shared/types'
import { useEffect, useState } from 'react'

const INITIAL: TransportSnapshot = {
  active: null,
  targetTransport: null,
  targetMode: null,
  switchPending: false,
  dongleDetected: false,
  wiredPhoneDetected: false,
  wirelessPhoneDetected: false,
  wirelessPhoneActive: false,
  wiredPhoneActive: false
}

type TransportEventHandler = (evt: unknown, ...args: unknown[]) => void
type TransportApi = {
  ipc?: {
    getTransportState?(): Promise<TransportSnapshot>
    onEvent?(handler: TransportEventHandler): unknown
    offEvent?(handler: TransportEventHandler): void
  }
}

export function useTransportState(): TransportSnapshot {
  const [state, setState] = useState<TransportSnapshot>(INITIAL)

  useEffect(() => {
    let cancelled = false

    const api = (window as unknown as { projection?: TransportApi }).projection
    if (!api?.ipc?.getTransportState || !api?.ipc?.onEvent) return

    api.ipc
      .getTransportState()
      .then((s: TransportSnapshot) => {
        if (!cancelled && s) setState(s)
      })
      .catch(() => {})

    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = args[0] as { type?: string; payload?: TransportSnapshot } | undefined
      if (msg?.type !== 'transportState' || !msg.payload) return
      setState(msg.payload)
    }
    api.ipc.onEvent(handler)

    return () => {
      cancelled = true
      api.ipc?.offEvent?.(handler)
    }
  }, [])

  return state
}
