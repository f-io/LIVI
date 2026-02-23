import { app } from 'electron'
import { join, basename, dirname } from 'path'
import { promises as fsp } from 'fs'
import { sendUpdateEvent } from '@main/ipc/utils'
import { spawn } from 'child_process'

export async function installOnLinuxFromFile(appImagePath: string): Promise<void> {
  if (process.platform !== 'linux') throw new Error('Linux only')
  const current = process.env.APPIMAGE
  if (!current) throw new Error('Not running from an AppImage')

  const currentDir = dirname(current)
  const currentBase = basename(current)

  const destNew = join(currentDir, currentBase + '.new')
  await fsp.copyFile(appImagePath, destNew)
  await fsp.chmod(destNew, 0o755)
  await fsp.rename(destNew, current)

  sendUpdateEvent({ phase: 'relaunching' })

  const cleanEnv: Record<string, string | undefined> = { ...process.env }
  delete cleanEnv.APPIMAGE
  delete cleanEnv.APPDIR
  delete cleanEnv.ARGV0
  delete cleanEnv.OWD

  const child = spawn(current, [], { detached: true, stdio: 'ignore', env: cleanEnv })
  child.unref()
  app.quit()
}
