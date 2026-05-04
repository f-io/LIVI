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
  lastConnectedAaBtMac?: string
  cameraMirror: boolean
  bindings: KeyBindings
  audioVolume: number
  navVolume: number
  voiceAssistantVolume: number
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
  voiceAssistant: string
  voiceAssistantRelease: string
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
  phoneKey0: 'Digit0',
  phoneKey1: 'Digit1',
  phoneKey2: 'Digit2',
  phoneKey3: 'Digit3',
  phoneKey4: 'Digit4',
  phoneKey5: 'Digit5',
  phoneKey6: 'Digit6',
  phoneKey7: 'Digit7',
  phoneKey8: 'Digit8',
  phoneKey9: 'Digit9',
  phoneKeyStar: '',
  phoneKeyHash: '',
  phoneKeyHookSwitch: '',

  // Voice / UI
  voiceAssistant: 'KeyV',
  voiceAssistantRelease: ''
}
