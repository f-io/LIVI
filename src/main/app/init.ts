import { app } from 'electron'

export function setupAppIdentity() {
  app.commandLine.appendSwitch('disable-renderer-backgrounding')
}
