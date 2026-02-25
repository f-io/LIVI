import { getMacDesiredOwner, sendUpdateEvent } from '@main/ipc/utils'
import { execFile } from 'node:child_process'
import { join } from 'path'
import { promises as fsp } from 'fs'

export async function installFromDmg(dmgPath: string): Promise<void> {
  if (process.platform !== 'darwin') throw new Error('macOS only')
  const mountPoint = `/Volumes/pcu-${Date.now()}`
  sendUpdateEvent({ phase: 'mounting' })
  await new Promise<void>((resolve, reject) =>
    execFile('hdiutil', ['attach', '-nobrowse', '-mountpoint', mountPoint, dmgPath], (err) =>
      err ? reject(err) : resolve()
    )
  )

  const entries = await fsp.readdir(mountPoint, { withFileTypes: true })
  const appFolder = entries.find(
    (e) => e.isDirectory() && e.name.toLowerCase().endsWith('.app')
  )?.name
  if (!appFolder) {
    await new Promise<void>((resolve) =>
      execFile('hdiutil', ['detach', mountPoint, '-quiet'], () => resolve())
    )
    throw new Error('No .app found in DMG')
  }

  const srcApp = join(mountPoint, appFolder)
  const dstApp = '/Applications/LIVI.app'
  const desired = await getMacDesiredOwner(dstApp)

  sendUpdateEvent({ phase: 'copying' })
  const script =
    `do shell script "set -e; dst=\\"${dstApp}\\"; src=\\"${srcApp}\\"; ` +
    `chflags -R nouchg,noschg $dst 2>/dev/null || true; rm -rf $dst; ` +
    `ditto -v $src $dst; xattr -cr $dst; chmod -RN $dst 2>/dev/null || true; ` +
    `chflags -R nouchg,noschg $dst 2>/dev/null || true; chown -R ${desired.user}:${desired.group} $dst" with administrator privileges`
  await new Promise<void>((resolve, reject) =>
    execFile('osascript', ['-e', script], (err) => (err ? reject(err) : resolve()))
  )

  sendUpdateEvent({ phase: 'unmounting' })
  await new Promise<void>((resolve) =>
    execFile('hdiutil', ['detach', mountPoint, '-quiet'], () => resolve())
  )
}
