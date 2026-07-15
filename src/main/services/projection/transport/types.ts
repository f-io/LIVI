type Device = USBDevice

export type Transport = 'dongle' | 'aa' | 'cp'

export type ConnectionMode = 'wired' | 'wireless'

export type Candidate = { transport: Transport; mode: ConnectionMode }

export const candidateEquals = (a: Candidate, b: Candidate): boolean =>
  a.transport === b.transport && a.mode === b.mode

export type { TransportSnapshot } from '@shared/types'

export type StartDecision =
  | { kind: 'none' }
  | { kind: 'start'; candidate: Candidate }
  | { kind: 'defer'; retryMs: number }

export type ArbiterDeps = {
  isWirelessEnabled: () => boolean
  isWirelessPhoneInRange: () => boolean
  getActiveTransport: () => Transport | null
  isDongleSessionActive: () => boolean
  isWiredAaSessionActive: () => boolean
  isWiredCpSessionActive: () => boolean
  onChange: () => void
  onShouldStop: () => Promise<void>
  onShouldAutoStart: () => void
  onShouldBringUpWiredBeside: () => void
  onWiredPhoneGone: () => void
}

export type WiredPhone = {
  device: Device | null
}
