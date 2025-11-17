import { DongleConfig } from '@carplay/messages'

export type ExtraConfig = DongleConfig & {
  kiosk: boolean
  camera: string
  microphone: string
  bindings: KeyBindings
  audioVolume: number
  navVolume: number
  siriVolume: number
  callVolume: number
  visualAudioDelayMs?: number
  primaryColorDark?: string
  primaryColorLight?: string
  highlightEditableFieldLight?: string
  highlightEditableFieldDark?: string
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

export interface CanMessage {
  canId: number
  byte: number
  mask: number
}

export interface CanConfig {
  reverse?: CanMessage
  lights?: CanMessage
}
