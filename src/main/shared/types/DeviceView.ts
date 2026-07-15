/** A device tile in the unified picker: native registry entry or dongle device.
 *  The single cross-boundary shape — main builds it, the renderer only mirrors it. */
export interface DeviceView {
  id: string
  name?: string
  model?: string
  protocol?: 'carplay' | 'androidauto'
  lastTransport?: string
  status: 'active' | 'available' | 'offline'
  source?: 'native' | 'dongle'
  batteryLevel?: number
  batteryCharging?: boolean
  signalStrength?: number
  carrierName?: string
  session?: number
}
