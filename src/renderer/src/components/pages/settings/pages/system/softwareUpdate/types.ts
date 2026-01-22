export enum UpdatePhases {
  'start' = 'start',
  'download' = 'download',
  'ready' = 'ready',
  'mounting' = 'mounting',
  'copying' = 'copying',
  'unmounting' = 'unmounting',
  'installing' = 'installing',
  'relaunching' = 'relaunching',
  'error' = 'error'
}

export const phaseMap: Record<UpdatePhases, string> = {
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
