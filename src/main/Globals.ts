import { DongleConfig } from '@main/services/carplay/messages'

export type TelemetryDashboardId = 'dash1' | 'dash2' | 'dash3' | 'dash4'

export type TelemetryDashboardConfig = {
  id: TelemetryDashboardId
  enabled: boolean
  pos: number
}

export type ExtraConfig = DongleConfig & {
  startPage: 'home' | 'media' | 'maps' | 'telemetry' | 'camera' | 'settings'
  kiosk: boolean
  camera: string
  telemetryEnabled: boolean
  telemetryDashboards?: TelemetryDashboardConfig[]
  cameraMirror: boolean
  bindings: KeyBindings
  audioVolume: number
  navVolume: number
  siriVolume: number
  callVolume: number
  autoSwitchOnStream: boolean
  autoSwitchOnPhoneCall: boolean
  autoSwitchOnGuidance: boolean
  visualAudioDelayMs: number
  primaryColorDark?: string
  primaryColorLight?: string
  highlightColorLight?: string
  highlightColorDark?: string
  dongleIcon120?: string
  dongleIcon180?: string
  dongleIcon256?: string
  language: string
}

export interface KeyBindings {
  selectUp: string
  selectDown: string
  up: string
  left: string
  right: string
  down: string
  back: string
  home: string
  playPause: string
  play: string
  pause: string
  next: string
  prev: string
  acceptPhone: string
  rejectPhone: string
  siri: string
}

export const DEFAULT_BINDINGS: KeyBindings = {
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  selectUp: '',
  selectDown: 'Enter',
  back: 'Backspace',
  home: 'KeyH',
  playPause: 'KeyP',
  play: '',
  pause: '',
  next: 'KeyN',
  prev: 'KeyB',
  acceptPhone: 'KeyA',
  rejectPhone: 'KeyR',
  siri: 'KeyV'
}
