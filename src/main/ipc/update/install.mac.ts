import { app } from 'electron'
import { installFromDmg } from '@main/ipc/update/install.dmg'
import { sendUpdateEvent } from '@main/ipc/utils'

export async function installOnMacFromFile(dmgPath: string): Promise<void> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  sendUpdateEvent({ phase: 'installing' })
  await installFromDmg(dmgPath)
  sendUpdateEvent({ phase: 'relaunching' })
  app.relaunch()
  setImmediate(() => app.quit())
}
