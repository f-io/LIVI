import { normalizeHttpUrl } from '@shared/utils'
import type { ServerResponse } from 'http'
import { createServer, request as httpRequest, type IncomingMessage, type Server } from 'http'
import { request as httpsRequest } from 'https'
import { connect as netConnect } from 'net'
import type { Duplex } from 'stream'
import { connect as tlsConnect } from 'tls'

/** Headers a framed page needs so a cross-origin isolated window may embed it. */
const FRAMING_HEADERS: Record<string, string> = {
  'cross-origin-embedder-policy': 'require-corp',
  'cross-origin-resource-policy': 'cross-origin'
}

const HOP_BY_HOP = [
  'connection',
  'keep-alive',
  'proxy-authenticate',
  'proxy-authorization',
  'te',
  'trailer',
  'transfer-encoding',
  'upgrade'
]

type Target = { hostname: string; port: number; secure: boolean; origin: string }

function parseTarget(url: string): Target | null {
  const normalized = normalizeHttpUrl(url)
  if (!normalized) return null

  const u = new URL(normalized)
  const secure = u.protocol === 'https:'
  return {
    hostname: u.hostname,
    port: Number(u.port) || (secure ? 443 : 80),
    secure,
    origin: u.origin
  }
}

/** Serves a remote address from a loopback port, adding the framing headers. */
export class CustomProxy {
  private server: Server | null = null
  private base: string | null = null
  private source = ''
  private starting: { source: string; done: Promise<string | null> } | null = null

  url(): string | null {
    return this.base
  }

  /** Resolves to the loopback address serving `source`, or null when it cannot. */
  async start(source: string): Promise<string | null> {
    const wanted = source || ''
    if (this.source === wanted && this.base) return this.base

    const pending = this.starting
    if (pending && pending.source === wanted) return pending.done

    const done = this.open(wanted)
    this.starting = { source: wanted, done }
    try {
      return await done
    } finally {
      if (this.starting?.done === done) this.starting = null
    }
  }

  private async open(source: string): Promise<string | null> {
    this.stop()

    const target = parseTarget(source)
    if (!target) {
      if (source) console.warn(`[custom-proxy] ${source} is no http address`)
      return null
    }

    const server = createServer((req, res) => this.forward(target, req, res))
    server.on('upgrade', (req, socket, head) => this.forwardUpgrade(target, req, socket, head))
    server.on('error', (e) => console.warn(`[custom-proxy] ${e.message}`))

    const base = await new Promise<string | null>((resolve) => {
      server.once('error', () => resolve(null))
      server.listen(0, '127.0.0.1', () => {
        const addr = server.address()
        resolve(typeof addr === 'object' && addr ? `http://127.0.0.1:${addr.port}/` : null)
      })
    })

    if (!base) {
      server.close()
      return null
    }

    this.server = server
    this.base = base
    this.source = source
    console.log(`[custom-proxy] ${base} -> ${target.origin}`)
    return base
  }

  stop(): void {
    this.server?.closeAllConnections()
    this.server?.close()
    this.server = null
    this.base = null
    this.source = ''
  }

  private forward(target: Target, req: IncomingMessage, res: ServerResponse): void {
    const headers: Record<string, string | string[]> = {}
    for (const [k, v] of Object.entries(req.headers)) {
      if (v !== undefined && !HOP_BY_HOP.includes(k)) headers[k] = v
    }
    headers.host = this.hostHeader(target)

    const send = target.secure ? httpsRequest : httpRequest
    const up = send(
      {
        hostname: target.hostname,
        port: target.port,
        path: req.url,
        method: req.method,
        headers,
        rejectUnauthorized: false
      },
      (remote) => {
        const out: Record<string, string | string[]> = {}
        for (const [k, v] of Object.entries(remote.headers)) {
          if (v !== undefined && !HOP_BY_HOP.includes(k)) out[k] = v
        }
        const location = remote.headers.location
        if (location) out.location = this.rewrite(target, location)
        Object.assign(out, FRAMING_HEADERS)
        res.writeHead(remote.statusCode ?? 502, out)
        remote.pipe(res)
      }
    )
    up.on('error', (e) => {
      res.writeHead(502, { 'content-type': 'text/plain; charset=utf-8' })
      res.end(e.message)
    })
    req.pipe(up)
  }

  private forwardUpgrade(target: Target, req: IncomingMessage, socket: Duplex, head: Buffer): void {
    const remote = target.secure
      ? tlsConnect({ host: target.hostname, port: target.port, rejectUnauthorized: false })
      : netConnect(target.port, target.hostname)

    remote.once(target.secure ? 'secureConnect' : 'connect', () => {
      const lines = [`${req.method} ${req.url} HTTP/1.1`]
      for (let i = 0; i < req.rawHeaders.length; i += 2) {
        const name = req.rawHeaders[i]
        const value =
          name.toLowerCase() === 'host' ? this.hostHeader(target) : req.rawHeaders[i + 1]
        lines.push(`${name}: ${value}`)
      }
      remote.write(`${lines.join('\r\n')}\r\n\r\n`)
      if (head?.length) remote.write(head)
      socket.pipe(remote).pipe(socket)
    })

    const drop = (): void => {
      remote.destroy()
      socket.destroy()
    }
    remote.on('error', drop)
    socket.on('error', drop)
  }

  private hostHeader(target: Target): string {
    const bare = target.secure ? target.port === 443 : target.port === 80
    return bare ? target.hostname : `${target.hostname}:${target.port}`
  }

  /** Keeps a redirect inside the proxy when it points back at the target. */
  private rewrite(target: Target, location: string): string {
    try {
      const abs = new URL(location, target.origin)
      if (abs.origin !== target.origin) return location
      return `${abs.pathname}${abs.search}${abs.hash}`
    } catch {
      return location
    }
  }
}

export const customProxy = new CustomProxy()
