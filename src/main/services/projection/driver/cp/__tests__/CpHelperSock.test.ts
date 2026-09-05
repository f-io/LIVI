import { EventEmitter } from 'node:events'
import type net from 'node:net'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { CpHelperSock, CpHelperSockError } from '../CpHelperSock'

const { createConnectionMock } = vi.hoisted(() => ({ createConnectionMock: vi.fn() }))

vi.mock('node:net', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:net')>()
  return { ...actual, default: actual, createConnection: createConnectionMock }
})

type FakeSock = EventEmitter & {
  write: ReturnType<typeof vi.fn>
  destroy: ReturnType<typeof vi.fn>
}

function fakeSock(): FakeSock {
  const s = new EventEmitter() as FakeSock
  s.write = vi.fn()
  s.destroy = vi.fn()
  return s
}

let sockets: FakeSock[]

beforeEach(() => {
  sockets = []
  createConnectionMock.mockReset()
  createConnectionMock.mockImplementation(() => {
    const s = fakeSock()
    sockets.push(s)
    return s as unknown as net.Socket
  })
})

afterEach(() => {
  vi.useRealTimers()
})

function reply(sock: FakeSock, obj: unknown): void {
  sock.emit('connect')
  sock.emit('data', Buffer.from(`${JSON.stringify(obj)}\n`))
}

function wrote(sock: FakeSock): string {
  sock.emit('connect')
  return sock.write.mock.calls[0]?.[0] as string
}

describe('CpHelperSock request', () => {
  it('writes the line on connect and resolves the parsed response', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    expect(wrote(sockets[0])).toBe('certificate\n')
    sockets[0].emit(
      'data',
      Buffer.from(
        `${JSON.stringify({ ok: true, data: Buffer.from('cert').toString('base64'), protocolMajor: 2 })}\n`
      )
    )
    await expect(p).resolves.toEqual(Buffer.from('cert'))
    await expect(helper.protocolMajor()).resolves.toBe(2)
    expect(sockets).toHaveLength(1)
  })

  it('reads the certificate once for protocolMajor and refuses to guess', async () => {
    const helper = new CpHelperSock()
    const p = helper.protocolMajor()
    reply(sockets[0], { ok: true, data: '' })
    await expect(p).rejects.toThrow('MFi auth protocol version unknown')
  })

  it('returns an empty certificate when the reply omits data', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    reply(sockets[0], { ok: true })
    await expect(p).resolves.toEqual(Buffer.alloc(0))
  })

  it('rejects certificate when the reply is not ok', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    reply(sockets[0], { ok: false, error: 'no chip' })
    await expect(p).rejects.toThrow('no chip')
  })

  it('signs a digest and returns the decoded data', async () => {
    const helper = new CpHelperSock()
    const p = helper.sign(Buffer.from('digest'))
    expect(wrote(sockets[0])).toBe(`sign ${Buffer.from('digest').toString('base64')}\n`)
    sockets[0].emit(
      'data',
      Buffer.from(`${JSON.stringify({ ok: true, data: Buffer.from('sig').toString('base64') })}\n`)
    )
    await expect(p).resolves.toEqual(Buffer.from('sig'))
  })

  it('returns an empty buffer when a data reply omits data', async () => {
    const helper = new CpHelperSock()
    const p = helper.sign(Buffer.from('d'))
    reply(sockets[0], { ok: true })
    await expect(p).resolves.toEqual(Buffer.alloc(0))
  })

  it('rejects a data request when the reply is not ok', async () => {
    const helper = new CpHelperSock()
    const p = helper.sign(Buffer.from('d'))
    reply(sockets[0], { ok: false, error: 'sign failed' })
    await expect(p).rejects.toThrow('sign failed')
  })

  it('rejects on malformed json', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('not-json\n'))
    await expect(p).rejects.toThrow(/bad json/)
  })

  it('waits for a full line before parsing', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('{"ok":true,'))
    sockets[0].emit('data', Buffer.from('"data":""}\n'))
    await expect(p).resolves.toEqual(Buffer.alloc(0))
  })

  it('rejects on socket error', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    sockets[0].emit('error', new Error('ECONNREFUSED'))
    await expect(p).rejects.toThrow(/cp-bt sock error: ECONNREFUSED/)
  })

  it('rejects when the socket closes without a response', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    sockets[0].emit('end')
    await expect(p).rejects.toThrow('cp-bt sock closed without response')
  })

  it('does not reject on end once already settled', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    reply(sockets[0], { ok: true, data: '' })
    await expect(p).resolves.toEqual(Buffer.alloc(0))
    expect(() => sockets[0].emit('end')).not.toThrow()
  })

  it('ignores a second data burst after settling', async () => {
    const helper = new CpHelperSock()
    const p = helper.certificate()
    sockets[0].emit('connect')
    sockets[0].emit('data', Buffer.from('{"ok":true,"data":""}\n'))
    await expect(p).resolves.toEqual(Buffer.alloc(0))
    expect(() => sockets[0].emit('data', Buffer.from('{"ok":true}\n'))).not.toThrow()
  })

  it('resolves even when destroy throws while settling', async () => {
    const helper = new CpHelperSock()
    sockets = []
    createConnectionMock.mockImplementationOnce(() => {
      const s = fakeSock()
      s.destroy = vi.fn(() => {
        throw new Error('already gone')
      })
      sockets.push(s)
      return s as unknown as net.Socket
    })
    const p = helper.disconnectBt('AA:BB')
    reply(sockets[0], { ok: true })
    await expect(p).resolves.toBeUndefined()
  })

  it('rejects with a timeout when no reply arrives', async () => {
    vi.useFakeTimers()
    const helper = new CpHelperSock()
    const p = helper.certificate()
    const assertion = expect(p).rejects.toThrow(/timeout after 8000ms/)
    vi.advanceTimersByTime(8000)
    await assertion
  })
})

describe('CpHelperSock rpc wrappers', () => {
  it('disconnectBt resolves on ok and throws otherwise', async () => {
    const helper = new CpHelperSock()
    const ok = helper.disconnectBt('AA:BB')
    expect(wrote(sockets[0])).toBe('disconnect AA:BB\n')
    sockets[0].emit('data', Buffer.from('{"ok":true}\n'))
    await expect(ok).resolves.toBeUndefined()

    const bad = helper.disconnectBt('CC:DD')
    reply(sockets[1], { ok: false, error: 'no device' })
    await expect(bad).rejects.toThrow('no device')
  })

  it('sendLocation is best-effort and base64-encodes the nmea', async () => {
    const helper = new CpHelperSock()
    const p = helper.sendLocation('$GPGGA')
    expect(wrote(sockets[0])).toBe(`location ${Buffer.from('$GPGGA', 'utf8').toString('base64')}\n`)
    sockets[0].emit('data', Buffer.from('{"ok":false,"error":"no subscriber"}\n'))
    await expect(p).resolves.toBeUndefined()
  })

  it('sendVehicleStatus serialises the status object', async () => {
    const helper = new CpHelperSock()
    const status = { range: 120, outsideTemperature: 21, rangeWarning: true }
    const p = helper.sendVehicleStatus(status)
    expect(wrote(sockets[0])).toBe(`vehicle-status ${JSON.stringify(status)}\n`)
    sockets[0].emit('data', Buffer.from('{"ok":true}\n'))
    await expect(p).resolves.toBeUndefined()
  })

  it('sendReconnectTargets serialises the targets in paging order', async () => {
    const helper = new CpHelperSock()
    const targets: Array<[string, string | null]> = [
      ['aa:bb', '10.0.0.2'],
      ['cc:dd', null]
    ]
    const p = helper.sendReconnectTargets(targets)
    expect(wrote(sockets[0])).toBe(`reconnect-targets ${JSON.stringify(targets)}\n`)
    sockets[0].emit('data', Buffer.from('{"ok":true}\n'))
    await expect(p).resolves.toBeUndefined()
  })

  it('setAaWireless sends 1 when enabled and throws on failure', async () => {
    const helper = new CpHelperSock()
    const on = helper.setAaWireless(true)
    expect(wrote(sockets[0])).toBe('set-aa 1\n')
    sockets[0].emit('data', Buffer.from('{"ok":true}\n'))
    await expect(on).resolves.toBeUndefined()

    const off = helper.setAaWireless(false)
    expect(wrote(sockets[1])).toBe('set-aa 0\n')
    sockets[1].emit('data', Buffer.from('{"ok":false,"error":"busy"}\n'))
    await expect(off).rejects.toThrow('busy')
  })

  it('setCpWireless sends 1 when enabled and throws on failure', async () => {
    const helper = new CpHelperSock()
    const on = helper.setCpWireless(true)
    expect(wrote(sockets[0])).toBe('set-cp 1\n')
    sockets[0].emit('data', Buffer.from('{"ok":true}\n'))
    await expect(on).resolves.toBeUndefined()

    const off = helper.setCpWireless(false)
    expect(wrote(sockets[1])).toBe('set-cp 0\n')
    sockets[1].emit('data', Buffer.from('{"ok":false,"error":"busy"}\n'))
    await expect(off).rejects.toThrow('busy')
  })

  it('CpHelperSockError carries its name', () => {
    expect(new CpHelperSockError('x').name).toBe('CpHelperSockError')
  })
})

describe('CpHelperSock subscribeEvents', () => {
  it('subscribes, fires onConnect, and parses whole and split push lines', () => {
    const helper = new CpHelperSock()
    const events: Record<string, unknown>[] = []
    const onConnect = vi.fn()
    const sub = helper.subscribeEvents((ev) => events.push(ev), onConnect)
    sockets[0].emit('connect')
    expect(sockets[0].write).toHaveBeenCalledWith('subscribe\n')
    expect(onConnect).toHaveBeenCalled()

    sockets[0].emit('data', Buffer.from('{"type":"wifi"}\n\n{"type":"dev'))
    sockets[0].emit('data', Buffer.from('ice"}\nnot-json\n'))
    expect(events).toEqual([{ type: 'wifi' }, { type: 'device' }])
    sub.close()
  })

  it('works without an onConnect callback', () => {
    const helper = new CpHelperSock()
    const sub = helper.subscribeEvents(() => {})
    expect(() => sockets[0].emit('connect')).not.toThrow()
    sub.close()
  })

  it('reconnects a second later after the socket closes', () => {
    vi.useFakeTimers()
    const helper = new CpHelperSock()
    const sub = helper.subscribeEvents(() => {})
    expect(sockets).toHaveLength(1)
    sockets[0].emit('error', new Error('reset'))
    sockets[0].emit('close')
    vi.advanceTimersByTime(1000)
    expect(sockets).toHaveLength(2)
    sub.close()
  })

  it('stops reconnecting once closed', () => {
    vi.useFakeTimers()
    const helper = new CpHelperSock()
    const sub = helper.subscribeEvents(() => {})
    sub.close()
    expect(sockets[0].destroy).toHaveBeenCalled()
    sockets[0].emit('close')
    vi.advanceTimersByTime(2000)
    expect(sockets).toHaveLength(1)
  })

  it('does not reconnect when closed before the retry timer fires', () => {
    vi.useFakeTimers()
    const helper = new CpHelperSock()
    const sub = helper.subscribeEvents(() => {})
    sockets[0].emit('close')
    sub.close()
    vi.advanceTimersByTime(1000)
    expect(sockets).toHaveLength(1)
  })

  it('tolerates a destroy that throws on close', () => {
    const helper = new CpHelperSock()
    sockets = []
    createConnectionMock.mockImplementationOnce(() => {
      const s = fakeSock()
      s.destroy = vi.fn(() => {
        throw new Error('already gone')
      })
      sockets.push(s)
      return s as unknown as net.Socket
    })
    const sub = helper.subscribeEvents(() => {})
    expect(() => sub.close()).not.toThrow()
  })
})
