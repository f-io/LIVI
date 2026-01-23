import { DongleConfig } from '@carplay/messages'

export type ExtraConfig = DongleConfig & {
  kiosk: boolean
  camera: string
  cameraMirror: boolean
  bindings: KeyBindings
  audioVolume: number
  navVolume: number
  siriVolume: number
  callVolume: number
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
