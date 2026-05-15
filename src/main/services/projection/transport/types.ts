import type { Device } from 'usb'

export type Transport = 'dongle' | 'aa'

export type ConnectionPreference = 'auto' | 'dongle' | 'native'

export type TransportSnapshot = {
  active: Transport | null
  dongleDetected: boolean
  nativeDetected: boolean
  preference: ConnectionPreference
}

export type StartDecision =
  | { kind: 'none' }
  | { kind: 'start'; transport: Transport }
  | { kind: 'defer'; retryMs: number }

export type ArbiterDeps = {
  getPreference: () => ConnectionPreference
  isAaEligible: () => boolean
  getActiveTransport: () => Transport | null
  isDongleSessionActive: () => boolean
  isWiredAaSessionActive: () => boolean
  onChange: () => void
  onShouldStop: () => Promise<void>
  onShouldAutoStart: () => void
}

export type WiredPhone = {
  device: Device | null
}
