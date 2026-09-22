import { CustomProxy } from '@main/services/custom/CustomProxy'
import { createServer, get, type Server } from 'http'
import type { AddressInfo } from 'net'
import { connect } from 'net'
import type { Duplex } from 'stream'

type Origin = {
  server: Server
  port: number
  seen: { url?: string; host?: string }
  sockets: Duplex[]
}

function startOrigin(
  handler?: (url: string) => { status?: number; headers?: Record<string, string>; body?: string }
): Promise<Origin> {
  const seen: Origin['seen'] = {}
  const sockets: Duplex[] = []
  let markClosed: () => void = () => undefined
  const closed = new Promise<void>((r) => {
    markClosed = () => r()
  })
  const server = createServer((req, res) => {
    seen.url = req.url
    seen.host = req.headers.host
    const out = handler?.(req.url ?? '') ?? {}
    res.writeHead(out.status ?? 200, { 'content-type': 'text/plain', ...(out.headers ?? {}) })
    res.end(out.body ?? 'origin')
  })
  server.on('upgrade', (_req, socket, head) => {
    sockets.push(socket)
    socket.on('error', () => undefined)
    socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n')
    socket.write('hello-from-origin')
    if (head?.length) socket.write(head)
    socket.on('data', (chunk) => socket.write(chunk))
    socket.on('close', () => markClosed())
  })
  return new Promise((resolve) =>
    server.listen(0, '127.0.0.1', () =>
      resolve({ server, port: (server.address() as AddressInfo).port, seen, sockets, closed })
    )
  )
}

function fetchText(
  url: string
): Promise<{ status: number; headers: Record<string, unknown>; body: string }> {
  return new Promise((resolve, reject) => {
    get(url, (res) => {
      let body = ''
      res.on('data', (c) => (body += c))
      res.on('end', () =>
        resolve({
          status: res.statusCode ?? 0,
          headers: res.headers as Record<string, unknown>,
          body
        })
      )
    }).on('error', reject)
  })
}

describe('CustomProxy', () => {
  const proxy = new CustomProxy()
  let origin: Origin | undefined

  afterEach(async () => {
    proxy.stop()
    for (const socket of origin?.sockets ?? []) socket.destroy()
    origin?.server.closeAllConnections()
    await new Promise((r) => origin?.server.close(r) ?? r(undefined))
    origin = undefined
    vi.restoreAllMocks()
  })

  test('serves the target and adds the framing headers', async () => {
    origin = await startOrigin(() => ({ body: 'remote page' }))
    const base = await proxy.start(`http://127.0.0.1:${origin.port}/`)

    const res = await fetchText(`${base}some/path?q=1`)

    expect(res.body).toBe('remote page')
    expect(res.headers['cross-origin-embedder-policy']).toBe('require-corp')
    expect(res.headers['cross-origin-resource-policy']).toBe('cross-origin')
    expect(origin.seen.url).toBe('/some/path?q=1')
    expect(origin.seen.host).toBe(`127.0.0.1:${origin.port}`)
  })

  test('keeps a redirect back to the target inside the proxy', async () => {
    origin = await startOrigin((url) =>
      url === '/away'
        ? { status: 302, headers: { location: `http://127.0.0.1:${origin?.port}/here` } }
        : { body: 'target' }
    )
    const base = await proxy.start(`http://127.0.0.1:${origin.port}/`)

    const res = await fetchText(`${base}away`)

    expect(res.status).toBe(302)
    expect(res.headers.location).toBe('/here')
  })

  test('leaves a redirect to a foreign origin alone', async () => {
    origin = await startOrigin(() => ({
      status: 302,
      headers: { location: 'http://example.invalid/elsewhere' }
    }))
    const base = await proxy.start(`http://127.0.0.1:${origin.port}/`)

    expect((await fetchText(`${base}x`)).headers.location).toBe('http://example.invalid/elsewhere')
  })

  test('passes a websocket upgrade through', async () => {
    origin = await startOrigin()
    const base = await proxy.start(`http://127.0.0.1:${origin.port}/`)
    const port = Number(new URL(base as string).port)

    const answer = await new Promise<string>((resolve, reject) => {
      const socket = connect(port, '127.0.0.1', () => {
        socket.write(
          'GET /live HTTP/1.1\r\nHost: proxy\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n'
        )
      })
      let seen = ''
      socket.on('data', (c) => {
        seen += c
        if (seen.includes('hello-from-origin')) {
          socket.destroy()
          resolve(seen)
        }
      })
      socket.on('error', reject)
    })

    expect(answer).toContain('101 Switching Protocols')
    expect(answer).toContain('hello-from-origin')
  })

  test('carries bytes that arrive with the upgrade', async () => {
    origin = await startOrigin()
    const base = await proxy.start(`http://127.0.0.1:${origin.port}/`)
    const port = Number(new URL(base as string).port)

    const answer = await new Promise<string>((resolve, reject) => {
      const socket = connect(port, '127.0.0.1', () => {
        socket.write(
          'GET /live HTTP/1.1\r\nHost: proxy\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\nEARLY'
        )
      })
      let seen = ''
      socket.on('data', (c) => {
        seen += c
        if (seen.includes('EARLY')) {
          socket.destroy()
          resolve(seen)
        }
      })
      socket.on('error', reject)
    })

    expect(answer).toContain('EARLY')
  })

  test('answers 502 when the target refuses', async () => {
    origin = await startOrigin()
    const dead = origin.port
    origin.server.closeAllConnections()
    await new Promise((r) => origin?.server.close(r))
    origin = undefined

    const base = await proxy.start(`http://127.0.0.1:${dead}/`)

    expect((await fetchText(`${base}x`)).status).toBe(502)
  })

  test('drops the socket when the upgrade target refuses', async () => {
    origin = await startOrigin()
    const dead = origin.port
    origin.server.closeAllConnections()
    await new Promise((r) => origin?.server.close(r))
    origin = undefined

    const base = await proxy.start(`http://127.0.0.1:${dead}/`)
    const port = Number(new URL(base as string).port)

    await new Promise<void>((resolve) => {
      const socket = connect(port, '127.0.0.1', () => {
        socket.write(
          'GET /live HTTP/1.1\r\nHost: p\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n'
        )
      })
      socket.on('close', () => resolve())
      socket.on('error', () => resolve())
    })
  })

  test('reads a bare host as http', async () => {
    origin = await startOrigin(() => ({ body: 'bare' }))
    const base = await proxy.start(`127.0.0.1:${origin.port}`)

    expect((await fetchText(`${base}x`)).body).toBe('bare')
  })

  test('concurrent starts of one address share the server', async () => {
    origin = await startOrigin()
    const source = `http://127.0.0.1:${origin.port}/`

    const [first, second] = await Promise.all([proxy.start(source), proxy.start(source)])

    expect(second).toBe(first)
    expect(proxy.url()).toBe(first)
  })

  test('a start overtaken by a newer one leaves it in place', async () => {
    origin = await startOrigin()
    const first = proxy.start(`http://127.0.0.1:${origin.port}/`)
    const second = proxy.start(`http://127.0.0.1:${origin.port}/other`)

    await first
    const running = await second

    expect(proxy.url()).toBe(running)
  })

  test('refuses an address that is no http url', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)

    expect(await proxy.start('ftp://somewhere/')).toBeNull()
    expect(await proxy.start('not a url')).toBeNull()
    expect(proxy.url()).toBeNull()
    expect(warn).toHaveBeenCalledTimes(2)
  })

  test('an empty address stops the proxy without complaining', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    origin = await startOrigin()
    await proxy.start(`http://127.0.0.1:${origin.port}/`)

    expect(await proxy.start('')).toBeNull()
    expect(proxy.url()).toBeNull()
    expect(warn).not.toHaveBeenCalled()
  })

  test('the same address keeps the running server', async () => {
    origin = await startOrigin()
    const source = `http://127.0.0.1:${origin.port}/`

    const first = await proxy.start(source)
    const again = await proxy.start(source)

    expect(again).toBe(first)
  })

  test('a new address moves the proxy', async () => {
    origin = await startOrigin()
    const first = await proxy.start(`http://127.0.0.1:${origin.port}/`)
    const second = await proxy.start(`http://127.0.0.1:${origin.port}/?x`)

    expect(second).not.toBe(first)
    expect(proxy.url()).toBe(second)
  })

  test('a location the url parser rejects is passed through', async () => {
    origin = await startOrigin(() => ({ status: 302, headers: { location: 'http://' } }))
    const base = await proxy.start(`http://127.0.0.1:${origin.port}/`)

    expect((await fetchText(`${base}x`)).headers.location).toBe('http://')
  })

  test('drops an upgrade to an https target that cannot be reached', async () => {
    const base = await proxy.start('https://127.0.0.1:1/')
    const port = Number(new URL(base as string).port)

    await new Promise<void>((resolve) => {
      const socket = connect(port, '127.0.0.1', () => {
        socket.write(
          'GET /l HTTP/1.1\r\nHost: p\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n'
        )
      })
      socket.on('close', () => resolve())
      socket.on('error', () => resolve())
    })
  })

  test('an https target keeps the bare host header', async () => {
    const base = await proxy.start('https://example.invalid/')
    const res = await fetchText(`${base}x`)

    expect(res.status).toBe(502)
  })
})
