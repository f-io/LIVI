import { useEffect, useState } from 'react'

export type TransportState = {
  active: 'dongle' | 'aa' | 'cp' | null
  targetTransport: 'dongle' | 'aa' | 'cp' | null
  targetMode: 'wired' | 'wireless' | null
  switchPending: boolean
  dongleDetected: boolean
  wiredPhoneDetected: boolean
  wirelessPhoneDetected: boolean
  wirelessPhoneActive: boolean
  wiredPhoneActive: boolean
  preference: 'auto' | 'dongle' | 'native'
}

const INITIAL: TransportState = {
  active: null,
  targetTransport: null,
  targetMode: null,
  switchPending: false,
  dongleDetected: false,
  wiredPhoneDetected: false,
  wirelessPhoneDetected: false,
  wirelessPhoneActive: false,
  wiredPhoneActive: false,
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
