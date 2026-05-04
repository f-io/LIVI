import { KeyCommand } from '../../components/worker/types'

export type BindKey =
  | 'left'
  | 'right'
  | 'up'
  | 'down'
  | 'back'
  | 'home'
  | 'selectDown'
  | 'selectUp'
  | 'next'
  | 'prev'
  | 'playPause'
  | 'play'
  | 'pause'
  | 'acceptPhone'
  | 'rejectPhone'
  | 'voiceAssistant'

export type useKeyDownProps = {
  receivingVideo: boolean
  inContainer: (navEl?: HTMLElement | null, el?: HTMLElement | null) => boolean
  focusSelectedNav: () => boolean
  focusFirstInMain: () => boolean
  moveFocusLinear: (delta: -1 | 1) => boolean
  isFormField: (el: HTMLElement | null) => boolean
  activateControl: (el: HTMLElement | null) => boolean
  onSetKeyCommand: (mappedAction: KeyCommand) => void
  onSetCommandCounter: (p: (_p: number) => number) => void
}
