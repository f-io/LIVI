import { EventEmitter } from 'node:events'
import { afterEach, describe, expect, test, vi } from 'vitest'

const { createConnectionMock, sockets } = vi.hoisted(() => {
  const sockets: FakeSocket[] = []
  const createConnectionMock = vi.fn((_path: string) => {
    const s = new FakeSocket()
    sockets.push(s)
    return s
  })
  return { createConnectionMock, sockets }
})

class FakeSocket extends EventEmitter {
  writable = true
  write = vi.fn()
  end = vi.fn()
}

vi.mock('node:net', () => ({ createConnection: createConnectionMock }))

import { HelperSessionLink } from '../HelperSessionLink'

function link(): { l: HelperSessionLink; sock: FakeSocket } {
  const sock = new FakeSocket()
  const l = new HelperSessionLink(sock as never, 'peer-1')
  return { l, sock }
}

/** Frames a message the way the helper would send it to this side. */
function messageFrame(ch: number, flags: number, msgId: number, payload: Buffer): Buffer {
  const head = Buffer.alloc(9)
  head.writeUInt32LE(5 + payload.length, 0)
  head.writeUInt8(0, 4)
  head.writeUInt8(ch, 5)
  head.writeUInt8(flags, 6)
  head.writeUInt16BE(msgId, 7)
  return Buffer.concat([head, payload])
}

function controlFrame(obj: unknown): Buffer {
  const body = Buffer.from(JSON.stringify(obj), 'utf8')
  const head = Buffer.alloc(5)
  head.writeUInt32LE(1 + body.length, 0)
  head.writeUInt8(1, 4)
  return Buffer.concat([head, body])
}

afterEach(() => {
  createConnectionMock.mockClear()
  sockets.length = 0
})

describe('HelperSessionLink', () => {
  test('connect resolves once the socket connects and keeps the peer label', async () => {
    const p = HelperSessionLink.connect('/tmp/aa.sock', 'peer-9')
    expect(createConnectionMock).toHaveBeenCalledWith('/tmp/aa.sock')
    sockets[0].emit('connect')
    const l = await p
    expect(l.peer).toBe('peer-9')
    expect(l.closed).toBe(false)
  })

  test('connect rejects when the socket errors first', async () => {
    const p = HelperSessionLink.connect('/tmp/aa.sock', 'peer-9')
    sockets[0].emit('error', new Error('refused'))
    await expect(p).rejects.toThrow('refused')
  })

  test('a socket close marks the link closed and re-emits it', () => {
    const { l, sock } = link()
    const closed = vi.fn()
    l.on('close', closed)
    sock.emit('close')
    expect(l.closed).toBe(true)
    expect(closed).toHaveBeenCalledTimes(1)
  })

  test('socket errors surface as link errors', () => {
    const { l, sock } = link()
    const onErr = vi.fn()
    l.on('error', onErr)
    sock.emit('error', new Error('boom'))
    expect(onErr).toHaveBeenCalledWith(expect.any(Error))
  })

  test('send frames a message and control frames json, both guarded when down', () => {
    const { l, sock } = link()
    l.send(3, 0x0b, 0x8004, Buffer.from([1, 2]))
    const wire = sock.write.mock.calls[0][0] as Buffer
    expect(wire.readUInt32LE(0)).toBe(7)
    expect(wire.readUInt8(4)).toBe(0)
    expect(wire.readUInt8(5)).toBe(3)
    expect(wire.readUInt8(6)).toBe(0x0b)
    expect(wire.readUInt16BE(7)).toBe(0x8004)
    expect(wire.subarray(9)).toEqual(Buffer.from([1, 2]))

    l.control({ type: 'sink', feed: '/tmp/f' })
    const ctrl = sock.write.mock.calls[1][0] as Buffer
    expect(ctrl.readUInt8(4)).toBe(1)
    expect(JSON.parse(ctrl.subarray(5).toString('utf8'))).toEqual({ type: 'sink', feed: '/tmp/f' })

    sock.writable = false
    l.send(0, 0, 0, Buffer.alloc(0))
    l.control({ type: 'x' })
    expect(sock.write).toHaveBeenCalledTimes(2)
  })

  test('end asks the helper to close the write side, destroy tears the link down once', () => {
    const { l, sock } = link()
    l.end()
    expect(
      JSON.parse((sock.write.mock.calls[0][0] as Buffer).subarray(5).toString('utf8'))
    ).toEqual({ type: 'end' })
    l.destroy()
    expect(
      JSON.parse((sock.write.mock.calls[1][0] as Buffer).subarray(5).toString('utf8'))
    ).toEqual({ type: 'close' })
    expect(sock.end).toHaveBeenCalledTimes(1)
    sock.emit('close')
    l.destroy()
    expect(sock.end).toHaveBeenCalledTimes(1)
  })

  test('incoming messages and controls are parsed, even split across chunks', () => {
    const { l, sock } = link()
    const messages: unknown[][] = []
    const controls: unknown[] = []
    l.on('message', (...a) => messages.push(a))
    l.on('control', (c) => controls.push(c))
    const wire = Buffer.concat([
      messageFrame(4, 0x0b, 1, Buffer.from([9, 8])),
      controlFrame({ type: 'ready', mic: '/tmp/m' })
    ])
    sock.emit('data', wire.subarray(0, 6))
    sock.emit('data', wire.subarray(6))
    expect(messages).toEqual([[4, 0x0b, 1, Buffer.from([9, 8])]])
    expect(controls).toEqual([{ type: 'ready', mic: '/tmp/m' }])
  })

  test('empty frames, unknown kinds, short messages and bad control json are skipped', () => {
    const { l, sock } = link()
    const messages: unknown[] = []
    const controls: unknown[] = []
    l.on('message', (...a) => messages.push(a))
    l.on('control', (c) => controls.push(c))

    const empty = Buffer.alloc(4) // len 0
    const unknownKind = Buffer.concat([Buffer.from([1, 0, 0, 0]), Buffer.from([9])]) // len 1, kind 9
    const shortMessage = Buffer.concat([Buffer.from([3, 0, 0, 0]), Buffer.from([0, 1, 2])])
    const badControl = Buffer.concat([
      Buffer.from([3, 0, 0, 0]),
      Buffer.from([1]),
      Buffer.from('{[')
    ])
    const nonObject = controlFrame(42)
    const noType = controlFrame({ foo: 1 })
    const nullObj = controlFrame(null)
    sock.emit(
      'data',
      Buffer.concat([empty, unknownKind, shortMessage, badControl, nonObject, noType, nullObj])
    )

    expect(messages).toHaveLength(0)
    expect(controls).toHaveLength(0)
  })

  test('a partial frame stays buffered until the rest arrives', () => {
    const { l, sock } = link()
    const controls: unknown[] = []
    l.on('control', (c) => controls.push(c))
    const wire = controlFrame({ type: 'closed' })
    sock.emit('data', wire.subarray(0, 3))
    expect(controls).toHaveLength(0)
    sock.emit('data', wire.subarray(3))
    expect(controls).toEqual([{ type: 'closed' }])
  })
})
