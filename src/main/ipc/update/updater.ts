import { installOnMacFromFile } from '@main/ipc/update/install.mac'
import { installOnLinuxFromFile } from '@main/ipc/update/install.linux'
import { pickAssetForPlatform } from '@main/ipc/update/pickAsset'
import { sendUpdateEvent, sendUpdateProgress } from '@main/ipc/utils'
import { join } from 'path'
import { downloadWithProgress } from '@main/ipc/update/downloader'
import { GhRelease, ServicesProps, UpdateSessionState } from '@main/types'
import { existsSync, promises as fsp } from 'fs'
import os from 'node:os'
import type { IpcMainInvokeEvent } from 'electron'

let updateSession: {
  state: UpdateSessionState
  tmpFile?: string
  cancel?: () => void
  platform?: 'darwin' | 'linux'
} = { state: 'idle' }

export class Updater {
  constructor(private services: ServicesProps) {}

  perform = async (_evt: IpcMainInvokeEvent, directUrl?: string) => {
    try {
      if (updateSession.state !== 'idle') throw new Error('Update already in progress')
      sendUpdateEvent({ phase: 'start' })

      const platform = process.platform
      if (platform !== 'darwin' && platform !== 'linux') {
        sendUpdateEvent({ phase: 'error', message: 'Unsupported platform' })
        return
      }
      updateSession.platform = platform as 'darwin' | 'linux'

      let url = directUrl
      if (!url) {
        const repo = process.env.UPDATE_REPO || 'f-io/LIVI'
        const feed =
          process.env.UPDATE_FEED || `https://api.github.com/repos/${repo}/releases/latest`
        const res = await fetch(feed, { headers: { 'User-Agent': 'LIVI-updater' } })
        if (!res.ok) throw new Error(`feed ${res.status}`)
        const json = (await res.json()) as unknown as GhRelease
        url = pickAssetForPlatform(json.assets || []).url
      }
      if (!url) throw new Error('No asset found for platform')

      const suffix = platform === 'darwin' ? '.dmg' : '.AppImage'
      const tmpFile = join(os.tmpdir(), `pcu-${Date.now()}${suffix}`)
      updateSession.tmpFile = tmpFile

      updateSession.state = 'downloading'
      const { promise, cancel } = downloadWithProgress(
        url,
        tmpFile,
        ({ received, total, percent }) => {
          sendUpdateProgress({ phase: 'download', received, total, percent })
        }
      )
      updateSession.cancel = () => {
        cancel()
        updateSession = { state: 'idle' }
        sendUpdateEvent({ phase: 'error', message: 'Aborted' })
      }

      await promise
      updateSession.state = 'ready'
      sendUpdateEvent({ phase: 'ready' })
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      updateSession = { state: 'idle' }
      sendUpdateEvent({ phase: 'error', message: msg })
    }
  }

  abort = async () => {
    try {
      if (updateSession.state === 'downloading' && updateSession.cancel) {
        updateSession.cancel()
      } else if (updateSession.state === 'ready') {
        if (updateSession.tmpFile && existsSync(updateSession.tmpFile)) {
          try {
            await fsp.unlink(updateSession.tmpFile)
          } catch {}
        }
      }
    } finally {
      updateSession = { state: 'idle' }
      sendUpdateEvent({ phase: 'error', message: 'Aborted' })
    }
  }

  install = async () => {
    try {
      if (updateSession.state !== 'ready' || !updateSession.tmpFile || !updateSession.platform) {
        throw new Error('No downloaded update ready')
      }

      try {
        await this.services.usbService.gracefulReset()
      } catch (e) {
        console.warn('[MAIN] gracefulReset failed (continuing install):', e)
      }

      await new Promise((r) => setTimeout(r, 150))

      const file = updateSession.tmpFile
      updateSession.state = 'installing'
      if (updateSession.platform === 'darwin') await installOnMacFromFile(file)
      else await installOnLinuxFromFile(file)
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      updateSession = { state: 'idle' }
      sendUpdateEvent({ phase: 'error', message: msg })
    }
  }
}
