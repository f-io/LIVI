import { registerIpcHandle } from '@main/ipc/register'
import fs from 'fs'
import path from 'path'
import { SendFile, SendLiviWeb, SendServerCgiScript } from '../messages/sendable'
import type { FirmwareCheckResult } from '../services/FirmwareUpdateService'
import type {
  DevToolsUploadResult,
  DongleFwRequest,
  DongleFwResponse,
  ProjectionIpcHost
} from './types'

function isRecord(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v)
}

function pickString(o: Record<string, unknown>, key: string): string | undefined {
  const v = o[key]
  return typeof v === 'string' ? v : undefined
}

function pickNumber(o: Record<string, unknown>, key: string): number | undefined {
  const v = o[key]
  return typeof v === 'number' ? v : undefined
}

function pickStringOrNumber(o: Record<string, unknown>, key: string): string | number | undefined {
  const v = o[key]
  return typeof v === 'string' || typeof v === 'number' ? v : undefined
}

type Deps = Pick<
  ProjectionIpcHost,
  | 'isStarted'
  | 'hasWebUsbDevice'
  | 'uploadIcons'
  | 'getDevToolsUrlCandidates'
  | 'send'
  | 'reloadConfigFromDisk'
  | 'getFirmware'
  | 'getApkVer'
  | 'getDongleFwVersion'
  | 'getBoxInfo'
  | 'emitProjectionEvent'
>

export function registerDongleIpc(host: Deps): void {
  registerIpcHandle('projection-upload-icons', async () => {
    if (!host.isStarted() || !host.hasWebUsbDevice()) {
      throw new Error('[ProjectionService] Projection is not started or dongle not connected')
    }
    host.uploadIcons()
  })

  registerIpcHandle('projection-upload-livi-scripts', async (): Promise<DevToolsUploadResult> => {
    if (!host.isStarted() || !host.hasWebUsbDevice()) {
      throw new Error('[ProjectionService] Projection is not started or dongle not connected')
    }

    const startedAtMs = Date.now()
    const startedAt = new Date(startedAtMs).toISOString()
    console.info('[projection-ipc] dev tools upload started')

    const cgiOk = await host.send(new SendServerCgiScript())
    const webOk = await host.send(new SendLiviWeb())
    const urls = host.getDevToolsUrlCandidates()

    const finishedAtMs = Date.now()
    const finishedAt = new Date(finishedAtMs).toISOString()
    const result: DevToolsUploadResult = {
      ok: Boolean(cgiOk && webOk),
      cgiOk: Boolean(cgiOk),
      webOk: Boolean(webOk),
      urls,
      startedAt,
      finishedAt,
      durationMs: finishedAtMs - startedAtMs
    }
    console.info('[projection-ipc] dev tools upload finished', result)
    return result
  })

  registerIpcHandle('dongle-fw', async (_evt, req: DongleFwRequest): Promise<DongleFwResponse> => {
    await host.reloadConfigFromDisk()

    const asError = (message: string): DongleFwResponse => ({
      ok: false,
      hasUpdate: false,
      size: 0,
      error: message,
      raw: { err: -1, msg: message }
    })

    const toRendererShape = (r: FirmwareCheckResult): DongleFwResponse => {
      if (!r.ok) return asError(r.error || 'Unknown error')
      const rawObj: Record<string, unknown> = isRecord(r.raw) ? r.raw : {}
      const rawErr = pickNumber(rawObj, 'err') ?? 0
      const rawToken = pickString(rawObj, 'token')
      const rawVer = pickString(rawObj, 'ver')
      const rawSize = pickStringOrNumber(rawObj, 'size')
      const rawId = pickString(rawObj, 'id')
      const rawNotes = pickString(rawObj, 'notes')
      const rawMsg = pickString(rawObj, 'msg')
      const rawError = pickString(rawObj, 'error')

      return {
        ok: true,
        hasUpdate: Boolean(r.hasUpdate),
        size: typeof r.size === 'number' ? r.size : 0,
        token: r.token,
        request: isRecord(r.request) ? r.request : undefined,
        raw: {
          err: rawErr,
          token: r.token ?? rawToken,
          ver: r.latestVer ?? rawVer,
          size: (typeof r.size === 'number' ? r.size : rawSize) ?? 0,
          id: r.id ?? rawId,
          notes: r.notes ?? rawNotes,
          msg: rawMsg,
          error: rawError
        }
      }
    }

    const fw = host.getFirmware()
    const fwQuery = () => ({
      appVer: host.getApkVer(),
      dongleFwVersion: host.getDongleFwVersion() ?? null,
      boxInfo: host.getBoxInfo()
    })

    const action = req?.action

    if (action === 'check') {
      host.emitProjectionEvent({ type: 'fwUpdate', stage: 'check:start' })
      const result = await fw.checkForUpdate(fwQuery())
      const shaped = toRendererShape(result)
      host.emitProjectionEvent({ type: 'fwUpdate', stage: 'check:done', result: shaped })
      return shaped
    }

    if (action === 'download') {
      try {
        host.emitProjectionEvent({ type: 'fwUpdate', stage: 'download:start' })
        const check = await fw.checkForUpdate(fwQuery())
        const shapedCheck = toRendererShape(check)

        if (!check.ok) {
          const msg = check.error || 'checkForUpdate failed'
          host.emitProjectionEvent({ type: 'fwUpdate', stage: 'download:error', message: msg })
          return asError(msg)
        }

        if (!check.hasUpdate) {
          host.emitProjectionEvent({
            type: 'fwUpdate',
            stage: 'download:done',
            path: null,
            bytes: 0
          })
          return shapedCheck
        }

        const dl = await fw.downloadFirmwareToHost(check, {
          overwrite: true,
          onProgress: (p) => {
            host.emitProjectionEvent({
              type: 'fwUpdate',
              stage: 'download:progress',
              received: p.received,
              total: p.total,
              percent: p.percent
            })
          }
        })

        if (!dl.ok) {
          const msg = dl.error || 'download failed'
          host.emitProjectionEvent({ type: 'fwUpdate', stage: 'download:error', message: msg })
          return asError(msg)
        }

        host.emitProjectionEvent({
          type: 'fwUpdate',
          stage: 'download:done',
          path: dl.path,
          bytes: dl.bytes
        })
        return shapedCheck
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        host.emitProjectionEvent({ type: 'fwUpdate', stage: 'download:error', message: msg })
        return asError(msg)
      }
    }

    if (action === 'upload') {
      try {
        if (!host.isStarted()) return asError('Projection not started / dongle not connected')

        host.emitProjectionEvent({ type: 'fwUpdate', stage: 'upload:start' })

        const st = await fw.getLocalFirmwareStatus(fwQuery())
        if (!st || st.ok !== true) {
          const msg = String(st?.error || 'Local firmware status failed')
          host.emitProjectionEvent({ type: 'fwUpdate', stage: 'upload:error', message: msg })
          return asError(msg)
        }

        if (!st.ready) {
          const msg = String(st.reason || 'No firmware ready to upload')
          host.emitProjectionEvent({ type: 'fwUpdate', stage: 'upload:error', message: msg })
          return asError(msg)
        }

        const fwBuf = await fs.promises.readFile(st.path)
        const remotePath = `/tmp/${path.basename(st.path)}`

        const ok = await host.send(new SendFile(fwBuf, remotePath))
        if (!ok) {
          const msg = 'Dongle upload failed (SendFile returned false)'
          host.emitProjectionEvent({ type: 'fwUpdate', stage: 'upload:error', message: msg })
          return asError(msg)
        }

        host.emitProjectionEvent({
          type: 'fwUpdate',
          stage: 'upload:file-sent',
          path: remotePath,
          bytes: fwBuf.length
        })
        return {
          ok: true,
          hasUpdate: true,
          size: fwBuf.length,
          token: undefined,
          request: { uploadedTo: remotePath, local: st },
          raw: { err: 0, msg: 'upload:file-sent', size: fwBuf.length }
        }
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        host.emitProjectionEvent({ type: 'fwUpdate', stage: 'upload:error', message: msg })
        return asError(msg)
      }
    }

    if (action === 'status') {
      const st = await fw.getLocalFirmwareStatus(fwQuery())
      if (!st) return asError('Local firmware status failed')
      if (st.ok !== true) {
        return asError(typeof st.error === 'string' ? st.error : 'Local firmware status failed')
      }

      if (!st.ready) {
        return {
          ok: true,
          hasUpdate: false,
          size: 0,
          token: undefined,
          request: { local: st },
          raw: { err: 0, msg: 'local:not-ready' }
        }
      }

      const latestVer = typeof st.latestVer === 'string' ? st.latestVer : undefined
      const bytes = st.bytes

      return {
        ok: true,
        hasUpdate: Boolean(latestVer),
        size: bytes,
        token: undefined,
        request: { local: st },
        raw: { err: 0, ver: latestVer, size: bytes, msg: 'local:ready' }
      }
    }

    return asError(`Unknown action: ${String(action)}`)
  })
}
