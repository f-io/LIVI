export type TransportSnapshot = {
  active: 'aa' | 'cp' | null
  targetTransport: 'aa' | 'cp' | null
  targetMode: 'wired' | 'wireless' | null
  switchPending: boolean
  wiredPhoneDetected: boolean
  wirelessPhoneDetected: boolean
  wiredPhoneActive: boolean
  wirelessPhoneActive: boolean
}
