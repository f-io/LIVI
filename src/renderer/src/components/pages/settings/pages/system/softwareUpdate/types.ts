export type UpdatePhase =
  | 'start'
  | 'download'
  | 'ready'
  | 'mounting'
  | 'copying'
  | 'unmounting'
  | 'installing'
  | 'relaunching'
  | 'error'

type PhaseText =
  | 'download'
  | 'installing'
  | 'mounting'
  | 'copying'
  | 'unmounting'
  | 'relaunching'
  | 'ready'
  | 'start'
  | 'error'

export const phaseMap: Record<PhaseText, string> = {
  download: 'Downloading',
  installing: 'Installing',
  mounting: 'Mounting image',
  copying: 'Copying',
  unmounting: 'Finalizing',
  relaunching: 'Relaunching',
  ready: 'Ready to install',
  start: 'Starting…',
  error: 'Error'
}

export enum UpgradeText {
  upgrade = 'Software Update',
  downgrade = 'Software Downgrade'
}
