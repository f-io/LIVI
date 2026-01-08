import { DongleConfig } from '@carplay/messages'

export type ExtraConfig = DongleConfig & {
  kiosk: boolean
  camera: string
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
  play: string
  pause: string
  next: string
  prev: string
}
