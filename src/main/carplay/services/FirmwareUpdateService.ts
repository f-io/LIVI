import { net, app } from 'electron'
import { join, basename } from 'path'
import { createWriteStream, existsSync } from 'fs'
import { promises as fsp } from 'fs'
import type { DongleDriver } from '../driver/DongleDriver.js'
import { SendTmpFile } from '../messages/sendable.js'

export type FirmwareCheckInput = {
  appVer: string
  lang?: number
  code?: number
  dongleFwVersion?: string | null
  boxInfo?: unknown
}

export type FirmwareCheckResult =
  | {
      ok: true
      hasUpdate: boolean
      latestVer?: string
      notes?: string
      size?: number
      id?: string
      token?: string
      request?: {
        lang: number
        code: number
        appVer: string
        ver: string
        uuid: string
        mfd: string
        fwn: string
        model: string
      }
      raw: unknown
    }
  | { ok: false; error: string }

type DevListEntry = {
  id?: string
  type?: string
  name?: string
  index?: string | number
  time?: string
  rfcomm?: string | number
}

type BoxInfoPayload = {
  uuid?: string
  MFD?: string
  boxType?: string
  OemName?: string
  productType?: string
  HiCar?: number
  supportLinkType?: string
  supportFeatures?: string
  hwVersion?: string
  WiFiChannel?: number
  CusCode?: string
  DevList?: DevListEntry[]
}

type FirmwareManifest = {
  createdAt: string
  path: string
  expectedSize: number
  latestVer?: string
  model: string
  uuid: string
  mfd: string
}

type LocalFirmwareStatus =
  | { ok: true; ready: true; path: string; bytes: number; model: string; latestVer?: string }
  | { ok: true; ready: false; reason: string }
  | { ok: false; error: string }

function normalizeBoxInfo(input: unknown): BoxInfoPayload | null {
  if (!input) return null
  if (typeof input === 'object') return input as BoxInfoPayload
  if (typeof input === 'string') {
    const s = input.trim()
    if (!s) return null
    try {
      const parsed = JSON.parse(s)
      if (parsed && typeof parsed === 'object') return parsed as BoxInfoPayload
    } catch {
      // ignore
    }
  }
  return null
}

function fmt(v: unknown): string | null {
  if (v == null) return null
  const s = String(v).trim()
  return s.length ? s : null
}

function toInt(v: unknown): number | null {
  if (v == null) return null
  const n = Number(v)
  return Number.isFinite(n) ? n : null
}

export class FirmwareUpdateService {
  private readonly apiUrl = 'http://api.paplink.cn/a/upgrade/checkBox'
  private readonly downUrl = 'http://api.paplink.cn/a/upgrade/down'

  public async checkForUpdate(input: FirmwareCheckInput): Promise<FirmwareCheckResult> {
    const lang = typeof input.lang === 'number' ? input.lang : 0
    const code = typeof input.code === 'number' ? input.code : 37
    const appVer = input.appVer

    const ver = fmt(input.dongleFwVersion)
    const box = normalizeBoxInfo(input.boxInfo)

    const uuid = fmt(box?.uuid)
    const mfd = fmt(box?.MFD)
    const model = fmt(box?.productType)

    if (!appVer) return { ok: false, error: 'Missing appVer' }
    if (!ver) return { ok: false, error: 'Missing dongleFwVersion (ver)' }
    if (!uuid) return { ok: false, error: 'Missing boxInfo.uuid' }
    if (!mfd) return { ok: false, error: 'Missing boxInfo.MFD' }
    if (!model) return { ok: false, error: 'Missing boxInfo.productType (model)' }

    const fwn = `${model}_Update.img`

    const form = new URLSearchParams()
    form.set('lang', String(lang))
    form.set('code', String(code))
    form.set('appVer', appVer)
    form.set('ver', ver)
    form.set('uuid', uuid)
    form.set('mfd', mfd)
    form.set('fwn', fwn)
    form.set('model', model)

    try {
      const rawText = await this.httpPostForm(this.apiUrl, form.toString())
      const raw = this.safeJson(rawText)
      const err = raw && typeof raw === 'object' ? (raw as any).err : null
      if (err !== 0) {
        return { ok: false, error: `checkBox err=${String(err ?? 'unknown')}` }
      }

      const latestVer = fmt((raw as any).ver)
      const notes = fmt((raw as any).notes)
      const size = toInt((raw as any).size)
      const id = fmt((raw as any).id)
      const token = fmt((raw as any).token)

      const hasUpdate = latestVer != null && latestVer !== ver

      return {
        ok: true,
        hasUpdate,
        latestVer: latestVer ?? undefined,
        notes: notes ?? undefined,
        size: size ?? undefined,
        id: id ?? undefined,
        token: token ?? undefined,
        request: { lang, code, appVer, ver, uuid, mfd, fwn, model },
        raw
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      return { ok: false, error: msg }
    }
  }

  public async downloadFirmwareToHost(
    check: Extract<FirmwareCheckResult, { ok: true }>,
    opts?: {
      onProgress?: (p: { received: number; total: number; percent: number }) => void
      overwrite?: boolean
    }
  ): Promise<{ ok: true; path: string; bytes: number } | { ok: false; error: string }> {
    try {
      const token = fmt(check.token)
      if (!token) return { ok: false, error: 'Missing token' }

      const req = check.request
      if (!req) return { ok: false, error: 'Missing request payload from checkForUpdate()' }

      const userData = app.getPath('userData')
      const fwDir = join(userData, 'firmware')
      await fsp.mkdir(fwDir, { recursive: true })

      const fileName = fmt(req.fwn) || 'Auto_Box_Update.img'
      const destPath = join(fwDir, fileName)
      const tmpPath = destPath + '.part'

      if (existsSync(destPath)) {
        if (opts?.overwrite) {
          try {
            await fsp.unlink(destPath)
          } catch {
            // ignore
          }
        } else {
          return { ok: false, error: `File already exists: ${destPath}` }
        }
      }

      try {
        await fsp.unlink(tmpPath)
      } catch {
        // ignore
      }

      const form = new URLSearchParams()
      form.set('lang', String(req.lang))
      form.set('code', String(req.code))
      form.set('appVer', req.appVer)
      form.set('ver', req.ver)
      form.set('uuid', req.uuid)
      form.set('mfd', req.mfd)
      form.set('fwn', req.fwn)
      form.set('model', req.model)

      const id = fmt(check.id)
      if (id) form.set('id', id)

      const result = await this.downloadToFile({
        url: this.downUrl,
        body: form.toString(),
        token,
        tmpPath,
        onProgress: opts?.onProgress
      })

      // Validate download size BEFORE renaming and writing manifest
      const expectedSize = typeof check.size === 'number' ? check.size : (toInt(check.size) ?? 0)
      if (expectedSize > 0 && result.bytes !== expectedSize) {
        try {
          await fsp.unlink(tmpPath)
        } catch {
          // ignore
        }
        return {
          ok: false,
          error: `Downloaded size mismatch (${result.bytes} != ${expectedSize})`
        }
      }

      await fsp.rename(tmpPath, destPath)

      const manifest: FirmwareManifest = {
        createdAt: new Date().toISOString(),
        path: destPath,
        expectedSize,
        latestVer: check.latestVer,
        model: req.model,
        uuid: req.uuid,
        mfd: req.mfd
      }

      await this.writeManifest(manifest)

      return { ok: true, path: destPath, bytes: result.bytes }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      return { ok: false, error: msg }
    }
  }

  public async startUpdate(
    input: FirmwareCheckInput,
    driver: DongleDriver,
    opts?: {
      onProgress?: (p: { sent: number; total: number; percent: number }) => void
      maxBytes?: number
    }
  ): Promise<{ ok: true } | { ok: false; error: string }> {
    try {
      // 1) validate local firmware file is present and matches current dongle
      const st = await this.getLocalFirmwareStatus(input)
      if (!st.ok) return { ok: false, error: st.error }
      if (!st.ready) return { ok: false, error: st.reason }

      const maxBytes = typeof opts?.maxBytes === 'number' ? opts.maxBytes : 50 * 1024 * 1024
      if (st.bytes > maxBytes) {
        return { ok: false, error: `Firmware file too large (${st.bytes} > ${maxBytes})` }
      }

      // 2) read file into memory (APK does the same; it pushes the image into /tmp)
      const buf = await fsp.readFile(st.path)

      // 3) keep original filename (A15W_Update.img, W15..., fallback, etc.)
      const fileName = basename(st.path)

      // 4) send to dongle /tmp/<fileName>
      // NOTE: This only uploads the image. The dongle-side update trigger command is still unknown.
      // With your wire logs, we can identify the trigger after you run one update.
      let sent = 0
      const total = buf.length

      // Simple single-shot send (fast). If you later discover size limits / chunking needs,
      // we can switch this to chunked sending.
      const ok = await driver.send(new SendTmpFile(buf, fileName))
      sent = total

      opts?.onProgress?.({ sent, total, percent: total > 0 ? sent / total : 1 })

      if (!ok) return { ok: false, error: 'Failed to send firmware image to dongle' }
      return { ok: true }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      return { ok: false, error: msg }
    }
  }

  private safeJson(text: string): unknown {
    try {
      return JSON.parse(text)
    } catch {
      return { err: -1, raw: text }
    }
  }

  private httpPostForm(url: string, body: string): Promise<string> {
    return new Promise((resolve, reject) => {
      const req = net.request({
        method: 'POST',
        url,
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' }
      })

      let data = ''
      req.on('response', (res) => {
        res.on('data', (chunk) => (data += chunk.toString('utf8')))
        res.on('end', () => resolve(data))
      })
      req.on('error', (err) => reject(err))

      req.write(body)
      req.end()
    })
  }

  private downloadToFile(input: {
    url: string
    body: string
    token: string
    tmpPath: string
    onProgress?: (p: { received: number; total: number; percent: number }) => void
  }): Promise<{ bytes: number }> {
    return new Promise((resolve, reject) => {
      let finished = false
      let received = 0
      let total = 0
      let out: ReturnType<typeof createWriteStream> | null = null

      const fail = async (err: unknown) => {
        if (finished) return
        finished = true
        try {
          out?.destroy()
        } catch {}
        out = null
        try {
          await fsp.unlink(input.tmpPath)
        } catch {
          // ignore
        }
        reject(err instanceof Error ? err : new Error(String(err)))
      }

      const req = net.request({
        method: 'POST',
        url: input.url,
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded',
          Authorization: input.token,
          OS: 'Android',
          'User-Agent': 'pi-carplay'
        }
      })

      req.on('response', (res) => {
        const status = res.statusCode ?? 0
        if (status !== 200) {
          void fail(new Error(`HTTP ${status}`))
          return
        }

        const cl = res.headers['content-length']
        if (typeof cl === 'string') total = parseInt(cl, 10) || 0
        else total = 0

        out = createWriteStream(input.tmpPath)

        out.on('error', (e) => void fail(e))
        res.on('error', (e) => void fail(e))

        res.on('data', (chunk: Buffer) => {
          try {
            received += chunk.length
            out?.write(chunk)
            if (input.onProgress) {
              input.onProgress({
                received,
                total,
                percent: total > 0 ? received / total : 0
              })
            }
          } catch (e) {
            void fail(e)
          }
        })

        res.on('end', () => {
          try {
            out?.end(() => {
              if (finished) return
              finished = true
              resolve({ bytes: received })
            })
          } catch (e) {
            void fail(e)
          }
        })
      })

      req.on('error', (e) => void fail(e))

      req.write(input.body)
      req.end()
    })
  }

  private async getFirmwareDir(): Promise<string> {
    const userData = app.getPath('userData')
    const fwDir = join(userData, 'firmware')
    await fsp.mkdir(fwDir, { recursive: true })
    return fwDir
  }

  private async writeManifest(m: FirmwareManifest): Promise<void> {
    const fwDir = await this.getFirmwareDir()
    const p = join(fwDir, 'dongle-fw.json')
    await fsp.writeFile(p, JSON.stringify(m, null, 2), 'utf8')
  }

  private async readManifest(): Promise<FirmwareManifest | null> {
    try {
      const fwDir = await this.getFirmwareDir()
      const p = join(fwDir, 'dongle-fw.json')
      const txt = await fsp.readFile(p, 'utf8')
      const parsed = JSON.parse(txt) as FirmwareManifest
      if (!parsed || typeof parsed !== 'object') return null
      return parsed
    } catch {
      return null
    }
  }

  public async getLocalFirmwareStatus(input: FirmwareCheckInput): Promise<LocalFirmwareStatus> {
    try {
      const box = normalizeBoxInfo(input.boxInfo)
      const modelNow = fmt(box?.productType)
      const uuidNow = fmt(box?.uuid)
      const mfdNow = fmt(box?.MFD)

      if (!modelNow) return { ok: true, ready: false, reason: 'Missing boxInfo.productType' }
      if (!uuidNow) return { ok: true, ready: false, reason: 'Missing boxInfo.uuid' }
      if (!mfdNow) return { ok: true, ready: false, reason: 'Missing boxInfo.MFD' }

      const manifest = await this.readManifest()
      if (!manifest) return { ok: true, ready: false, reason: 'No downloaded firmware manifest' }

      if (manifest.model !== modelNow) {
        return {
          ok: true,
          ready: false,
          reason: `Model mismatch (have ${modelNow}, expected ${manifest.model})`
        }
      }
      if (manifest.uuid !== uuidNow) {
        return { ok: true, ready: false, reason: 'Dongle UUID mismatch (dongle was swapped)' }
      }
      if (manifest.mfd !== mfdNow) {
        return { ok: true, ready: false, reason: 'Dongle MFD mismatch (dongle was swapped)' }
      }

      const expectedName = `${modelNow}_Update.img`
      const actualName = basename(manifest.path)
      if (actualName !== expectedName) {
        return {
          ok: true,
          ready: false,
          reason: `File name mismatch (${actualName} != ${expectedName})`
        }
      }

      const st = await fsp.stat(manifest.path).catch(() => null)
      if (!st || !st.isFile()) return { ok: true, ready: false, reason: 'Firmware file missing' }

      const bytes = st.size
      if (manifest.expectedSize > 0 && bytes !== manifest.expectedSize) {
        return {
          ok: true,
          ready: false,
          reason: `Size mismatch (${bytes} != ${manifest.expectedSize})`
        }
      }

      return {
        ok: true,
        ready: true,
        path: manifest.path,
        bytes,
        model: modelNow,
        latestVer: manifest.latestVer
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      return { ok: false, error: msg }
    }
  }
}
