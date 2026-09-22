import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

class MockSocket extends EventEmitter {
  writes: Buffer[] = []
  destroyed = false
  write(b: Buffer): boolean {
    this.writes.push(b)
    return true
  }
  destroy(): void {
    this.destroyed = true
  }
  connect(): void {
    // emitted by the test to drive the connect path
    this.emit('connect')
  }
}

const { sockets, createConnection } = vi.hoisted(() => {
  const list: MockSocket[] = []
  return {
    sockets: list,
    createConnection: vi.fn((_path: string) => {
      const s = new MockSocket()
      list.push(s)
      return s
    })
  }
})

vi.mock('node:net', () => ({ default: { createConnection }, createConnection }))
vi.mock('../assets/LIVI_cgi.js', () => ({ buildServerCgiScript: () => 'CGI' }))
vi.mock('../assets/LIVI_web.js', () => ({ buildLiviWeb: () => 'WEB' }))

import { DongleUpload } from '../dongleUpload'

const MAGIC = 0x55aa_55aa

/** Splits a stream of 0x55aa framed messages into [type, payload] pairs. */
function deframe(buf: Buffer): Array<{ type: number; payload: Buffer }> {
  const out: Array<{ type: number; payload: Buffer }> = []
  let o = 0
  while (o + 16 <= buf.length) {
    expect(buf.readUInt32LE(o)).toBe(MAGIC)
    const len = buf.readUInt32LE(o + 4)
    const type = buf.readUInt32LE(o + 8)
    expect(buf.readUInt32LE(o + 12)).toBe(~type >>> 0)
    out.push({ type, payload: buf.subarray(o + 16, o + 16 + len) })
    o += 16 + len
  }
  return out
}

function fakeHelper(): {
  subscribe: Mock
  announce: (ev: unknown) => void
  closeSub: () => void
} {
  let onEv: ((ev: unknown) => void) | null = null
  let onClose: (() => void) | null = null
  const subscribe = vi.fn((ev: (ev: unknown) => void, close: () => void) => {
    onEv = ev
    onClose = close
    return { close: vi.fn() }
  })
  return { subscribe, announce: (ev) => onEv?.(ev), closeSub: () => onClose?.() }
}

describe('DongleUpload', () => {
  beforeEach(() => {
    sockets.length = 0
    createConnection.mockClear()
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('reports unavailable and refuses to send with no dongle on the bus', async () => {
    const up = new DongleUpload()
    expect(up.available).toBe(false)
    await expect(up.uploadFile('/x', Buffer.from('y'))).resolves.toEqual({
      ok: false,
      error: 'no dongle on the bus'
    })
  })

  it('becomes available on a dongle-upload announce and ignores other events', () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)

    helper.announce({ event: 'aa-session', socket: '/tmp/aa.sock' })
    expect(up.available).toBe(false)

    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock', serial: 'S1' })
    expect(up.available).toBe(true)
  })

  it('uploadFile sends an Open handshake then one SendFile, framed', async () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock' })

    const p = up.uploadFile('/tmp/boa/x', Buffer.from('hello'))
    expect(createConnection).toHaveBeenCalledWith('/tmp/up.sock')
    sockets[0].connect()

    const frames = deframe(Buffer.concat(sockets[0].writes))
    expect(frames.map((f) => f.type)).toEqual([0x01, 0x99])
    // Open payload is 28 bytes; SendFile payload = [nameLen][name\0][contentLen][content]
    expect(frames[0].payload).toHaveLength(28)
    const sf = frames[1].payload
    const nameLen = sf.readUInt32LE(0)
    expect(sf.subarray(4, 4 + nameLen).toString('ascii')).toBe('/tmp/boa/x\0')
    const contentLen = sf.readUInt32LE(4 + nameLen)
    expect(sf.subarray(8 + nameLen, 8 + nameLen + contentLen).toString('utf8')).toBe('hello')

    await vi.runAllTimersAsync()
    await expect(p).resolves.toEqual({ ok: true })
    expect(sockets[0].destroyed).toBe(true)
  })

  it('bootstrap uploads server.cgi and index.html', async () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock' })

    const p = up.bootstrap()
    sockets[0].connect()
    const frames = deframe(Buffer.concat(sockets[0].writes))
    const names = frames
      .filter((f) => f.type === 0x99)
      .map((f) => {
        const nl = f.payload.readUInt32LE(0)
        return f.payload
          .subarray(4, 4 + nl)
          .toString('ascii')
          .replace('\0', '')
      })
    expect(names).toEqual(['/tmp/boa/cgi-bin/server.cgi', '/tmp/boa/www/index.html'])

    await vi.runAllTimersAsync()
    await expect(p).resolves.toEqual({ ok: true })
  })

  it('surfaces a connect error', async () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock' })

    const p = up.uploadFile('/x', Buffer.from('y'))
    sockets[0].emit('error', new Error('ECONNREFUSED'))
    await expect(p).resolves.toEqual({ ok: false, error: 'ECONNREFUSED' })
  })

  it('detachHelper clears availability', () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock' })
    expect(up.available).toBe(true)
    up.detachHelper()
    expect(up.available).toBe(false)
  })

  it('resolves with a timeout error when the socket never connects', async () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock' })

    const p = up.uploadFile('/x', Buffer.from('y'))
    await vi.advanceTimersByTimeAsync(3000)
    await expect(p).resolves.toEqual({ ok: false, error: 'connect timed out' })
    expect(sockets[0].destroyed).toBe(true)
  })

  it('drops the subscription when the helper stream closes', () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.closeSub()
    up.detachHelper() // no throw: sub is already gone
    expect(up.available).toBe(false)
  })

  it('attachHelper(undefined) leaves it detached', () => {
    const up = new DongleUpload()
    up.attachHelper(undefined)
    expect(up.available).toBe(false)
  })

  it('a late socket error after connect does not override the result', async () => {
    const up = new DongleUpload()
    const helper = fakeHelper()
    up.attachHelper(helper as never)
    helper.announce({ event: 'dongle-upload', socket: '/tmp/up.sock' })

    const p = up.uploadFile('/x', Buffer.from('y'))
    sockets[0].connect() // schedules the drain-close
    sockets[0].emit('error', new Error('late')) // first to finish wins
    await vi.advanceTimersByTimeAsync(300) // drain fires into the already-done guard
    await expect(p).resolves.toEqual({ ok: false, error: 'late' })
  })
})
