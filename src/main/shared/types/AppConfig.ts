import type { DongleConfig } from './DongleConfig'

export type TelemetryDashboardId = 'dash1' | 'dash2' | 'dash3' | 'dash4'

export type TelemetryDashboardConfig = {
  id: TelemetryDashboardId
  enabled: boolean
  pos: number
}

export type ExtraConfig = DongleConfig & {
  startPage: 'home' | 'media' | 'maps' | 'telemetry' | 'camera' | 'settings'
  language: string
  kiosk: boolean
  uiZoomPercent: number
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
  dongleToolsIp?: string
  primaryColorDark?: string
  primaryColorLight?: string
  highlightColorLight?: string
  highlightColorDark?: string
  dongleIcon120?: string
  dongleIcon180?: string
  dongleIcon256?: string
}

export type KeyBindings = {
  // D-PAD
  up: string
  down: string
  left: string
  right: string
  selectUp: string
  selectDown: string
  back: string

  // Rotary Knob
  knobLeft: string
  knobRight: string
  knobUp: string
  knobDown: string

  // Media Control
  home: string
  playPause: string
  play: string
  pause: string
  next: string
  prev: string

  // Phone
  acceptPhone: string
  rejectPhone: string
  phoneKey0: string
  phoneKey1: string
  phoneKey2: string
  phoneKey3: string
  phoneKey4: string
  phoneKey5: string
  phoneKey6: string
  phoneKey7: string
  phoneKey8: string
  phoneKey9: string
  phoneKeyStar: string
  phoneKeyHash: string
  phoneKeyHookSwitch: string

  // Voice
  siri: string
  siriRelease: string
}

export const DEFAULT_BINDINGS: KeyBindings = {
  // D-PAD
  up: 'ArrowUp',
  down: 'ArrowDown',
  left: 'ArrowLeft',
  right: 'ArrowRight',
  selectUp: '',
  selectDown: 'Enter',
  back: 'Backspace',

  // Rotary Knob
  knobLeft: '',
  knobRight: '',
  knobUp: '',
  knobDown: '',

  // Media Control
  home: 'KeyH',
  playPause: 'KeyP',
  play: '',
  pause: '',
  next: 'KeyN',
  prev: 'KeyB',

  // Phone
  acceptPhone: 'KeyA',
  rejectPhone: 'KeyR',
  phoneKey0: '',
  phoneKey1: '',
  phoneKey2: '',
  phoneKey3: '',
  phoneKey4: '',
  phoneKey5: '',
  phoneKey6: '',
  phoneKey7: '',
  phoneKey8: '',
  phoneKey9: '',
  phoneKeyStar: '',
  phoneKeyHash: '',
  phoneKeyHookSwitch: '',

  // Voice / UI
  siri: 'KeyV',
  siriRelease: ''
}
