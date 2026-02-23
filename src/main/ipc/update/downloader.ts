import { createWriteStream, existsSync, promises as fsp } from 'fs'
import * as https from 'node:https'

export async function downloadWithProgress(
  url: string,
  dest: string,
  onProgress: (p: { received: number; total: number; percent: number }) => void
  // FIXME
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore
): { promise: Promise<void>; cancel: () => void } {
  let req: import('http').ClientRequest | null = null
  let file: import('fs').WriteStream | null = null
  let resolved = false
  let rejected = false
  let cancelled = false
  let _resolve: (() => void) | null = null
  let _reject: ((e: unknown) => void) | null = null
  let redirectCancel: (() => void) | null = null

  const safeResolve = () => {
    if (!resolved && !rejected) {
      resolved = true
      _resolve?.()
    }
  }
  const safeReject = (e: unknown) => {
    if (!resolved && !rejected) {
      rejected = true
      _reject?.(e)
    }
  }

  const cleanup = async () => {
    try {
      req?.destroy()
    } catch {}
    req = null
    try {
      file?.destroy()
    } catch {}
    file = null
    try {
      if (existsSync(dest)) await fsp.unlink(dest).catch(() => {})
    } catch {}
  }

  const promise = new Promise<void>((resolve, reject) => {
    _resolve = resolve
    _reject = (e: unknown) => reject(e)

    req = https.get(url, (res) => {
      // Redirect
      if (res.statusCode && res.statusCode >= 300 && res.headers.location) {
        try {
          req!.destroy()
        } catch {}
        const next = downloadWithProgress(res.headers.location, dest, onProgress)
        redirectCancel = next.cancel
        next.promise.then(resolve, reject)
        req = null
        return
      }

      if (res.statusCode !== 200) {
        safeReject(new Error(`HTTP ${res.statusCode}`))
        return
      }

      const total = parseInt(String(res.headers['content-length'] || 0), 10) || 0
      let received = 0
      file = createWriteStream(dest)

      res.on('data', (chunk: Buffer) => {
        if (cancelled) return
        received += chunk.length
        onProgress({ received, total, percent: total ? received / total : 0 })
      })
      res.on('error', (err: Error) => {
        if (cancelled) return
        safeReject(err)
      })

      file.on('error', (err: Error) => {
        if (cancelled) return
        safeReject(err)
      })
      file.on('finish', async () => {
        if (cancelled) return
        try {
          await new Promise<void>((r) => file?.close(() => r()))
        } catch {}
        file = null
        safeResolve()
      })

      res.pipe(file)
    })

    req.on('error', (e: Error) => {
      if (cancelled) return
      safeReject(e)
    })
  })

  const cancel = () => {
    if (cancelled) return
    cancelled = true
    try {
      redirectCancel?.()
    } catch {}
    cleanup().finally(() => safeReject(new Error('aborted')))
  }

  return { promise, cancel }
}
