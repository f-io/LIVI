import type { DeviceView } from '@shared/types'
import { useEffect, useState } from 'react'

export type { DeviceView }

type EventHandler = (evt: unknown, ...args: unknown[]) => void
type DevicesApi = {
  ipc?: {
    getDevices?(): Promise<DeviceView[]>
    onEvent?(handler: EventHandler): unknown
    offEvent?(handler: EventHandler): void
    selectDevice?(id: string): Promise<{ ok: boolean }>
    forgetDevice?(id: string): Promise<{ ok: boolean }>
  }
}

function api(): DevicesApi | undefined {
  return (window as unknown as { projection?: DevicesApi }).projection
}

export function useDevices(): DeviceView[] {
  const [devices, setDevices] = useState<DeviceView[]>([])

  useEffect(() => {
    let cancelled = false
    const a = api()
    if (!a?.ipc?.getDevices || !a?.ipc?.onEvent) return

    a.ipc
      .getDevices()
      .then((d) => {
        if (!cancelled && d) setDevices(d)
      })
      .catch(() => {})

    const handler = (_evt: unknown, ...args: unknown[]) => {
      const msg = args[0] as { type?: string; payload?: DeviceView[] } | undefined
      if (msg?.type !== 'devices' || !msg.payload) return
      setDevices(msg.payload)
    }
    a.ipc.onEvent(handler)

    return () => {
      cancelled = true
      a.ipc?.offEvent?.(handler)
    }
  }, [])

  return devices
}

export function selectDevice(id: string): void {
  api()
    ?.ipc?.selectDevice?.(id)
    ?.catch?.(() => {})
}

export function forgetDevice(id: string): void {
  api()
    ?.ipc?.forgetDevice?.(id)
    ?.catch?.(() => {})
}
