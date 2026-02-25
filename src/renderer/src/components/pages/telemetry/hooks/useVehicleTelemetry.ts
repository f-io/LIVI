import * as React from 'react'
import { TelemetryPayload as VehicleTelemetry } from '@main/services/Socket'

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null
}

function asVehicleTelemetry(v: unknown): VehicleTelemetry | null {
  if (!isRecord(v)) return null
  return v as VehicleTelemetry
}

export function useVehicleTelemetry() {
  const [telemetry, setTelemetry] = React.useState<VehicleTelemetry | null>(null)
  const lastTsRef = React.useRef<number>(0)

  React.useEffect(() => {
    const onMsg = (payload: unknown) => {
      const msg = asVehicleTelemetry(payload)
      if (!msg) return

      // keep a monotonic timestamp
      const ts = typeof msg.ts === 'number' ? msg.ts : Date.now()
      lastTsRef.current = ts

      setTelemetry((prev) => ({ ...(prev ?? {}), ...msg, ts }))
    }

    window.carplay?.ipc?.onTelemetry?.(onMsg)

    return () => {
      window.carplay?.ipc?.offTelemetry?.()
    }
  }, [])

  const isStale = React.useMemo(() => {
    if (!telemetry?.ts) return true
    return Date.now() - telemetry.ts > 1500
  }, [telemetry?.ts])

  return { telemetry, isStale }
}
