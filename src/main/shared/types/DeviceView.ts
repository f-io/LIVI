/** A device tile in the unified picker, built from the native registry.
 *  The single cross-boundary shape — main builds it, the renderer only mirrors it. */
export interface DeviceView {
  id: string
  name?: string
  model?: string
  protocol?: 'carplay' | 'androidauto'
  lastTransport?: string
  status: 'active' | 'available' | 'offline'
  source?: 'native'
  batteryLevel?: number
  batteryCharging?: boolean
  signalStrength?: number
  carrierName?: string
  session?: number
}
