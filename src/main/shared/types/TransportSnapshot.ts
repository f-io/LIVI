export type TransportSnapshot = {
  active: 'dongle' | 'aa' | 'cp' | null
  targetTransport: 'dongle' | 'aa' | 'cp' | null
  targetMode: 'wired' | 'wireless' | null
  switchPending: boolean
  dongleDetected: boolean
  wiredPhoneDetected: boolean
  wirelessPhoneDetected: boolean
  wiredPhoneActive: boolean
  wirelessPhoneActive: boolean
}
