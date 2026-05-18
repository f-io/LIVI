import type { Device } from 'usb'

export type Transport = 'dongle' | 'aa' | 'cp'

export type ConnectionMode = 'wired' | 'wireless'

export type Candidate = { transport: Transport; mode: ConnectionMode }

export const candidateEquals = (a: Candidate, b: Candidate): boolean =>
  a.transport === b.transport && a.mode === b.mode

export type ConnectionPreference = 'auto' | 'dongle' | 'native'

export type TransportSnapshot = {
  active: Transport | null
  targetTransport: Transport | null
  targetMode: ConnectionMode | null
  switchPending: boolean
  dongleDetected: boolean
  wiredPhoneDetected: boolean
  wirelessPhoneDetected: boolean
  wiredPhoneActive: boolean
  wirelessPhoneActive: boolean
  preference: ConnectionPreference
}

export type StartDecision =
  | { kind: 'none' }
  | { kind: 'start'; candidate: Candidate }
  | { kind: 'defer'; retryMs: number }

export type ArbiterDeps = {
  getPreference: () => ConnectionPreference
  isWirelessEnabled: () => boolean
  isWirelessPhoneInRange: () => boolean
  getActiveTransport: () => Transport | null
  isDongleSessionActive: () => boolean
  isWiredAaSessionActive: () => boolean
  isWiredCpSessionActive: () => boolean
  onChange: () => void
  onShouldStop: () => Promise<void>
  onShouldAutoStart: () => void
}

export type WiredPhone = {
  device: Device | null
}
