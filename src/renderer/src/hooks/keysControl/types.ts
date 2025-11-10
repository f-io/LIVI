import { KeyCommand } from '../../components/worker/types'
import { ExtraConfig } from '@main/Globals'

export type BindKey =
  | 'left'
  | 'right'
  | 'up'
  | 'down'
  | 'back'
  | 'selectDown'
  | 'next'
  | 'prev'
  | 'play'
  | 'pause'
  | 'seekFwd'
  | 'seekBack'

export type useKeyDownProps = {
  settings: ExtraConfig | null
  receivingVideo: boolean
  inContainer: (navEl: HTMLDivElement | null, el: HTMLElement | null) => boolean
  focusSelectedNav: () => boolean
  focusFirstInMain: () => boolean
  moveFocusLinear: (delta: -1 | 1) => boolean
  isFormField: (el: HTMLElement | null) => boolean
  editingField: HTMLElement | null
  activateControl: (el: HTMLElement | null) => boolean
  navRef: React.RefObject<HTMLDivElement | null>
  mainRef: React.RefObject<HTMLDivElement | null>
  onSetKeyCommand: (mappedAction: KeyCommand) => void
  onSetCommandCounter: (p: (_p: number) => number) => void
  onSetEditingField: (el: HTMLElement | null) => void
}
