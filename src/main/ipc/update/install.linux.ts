import { promises as fsp } from 'fs'
import { basename, dirname, join } from 'path'

// Replace the running AppImage in place
export async function installOnLinuxFromFile(appImagePath: string): Promise<void> {
  if (process.platform !== 'linux') throw new Error('Linux only')
  const current = process.env.APPIMAGE
  if (!current) throw new Error('Not running from an AppImage')

  const destNew = join(dirname(current), basename(current) + '.new')
  await fsp.copyFile(appImagePath, destNew)
  await fsp.chmod(destNew, 0o755)
  await fsp.rename(destNew, current)
}
