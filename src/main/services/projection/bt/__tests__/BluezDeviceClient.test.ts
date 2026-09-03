import { EventEmitter } from 'node:events'

class MockSocket extends EventEmitter {
  write = vi.fn()
  destroy = vi.fn()
}

const createConnection = vi.fn()

vi.mock('net', () => ({
  __esModule: true,
  createConnection: (...args: unknown[]) => createConnection(...args)
}))

import { BluezDeviceClient, BluezDeviceError } from '../BluezDeviceClient'

function makeClient(): { client: BluezDeviceClient; nextSocket: () => MockSocket } {
  const sockets: MockSocket[] = []
  createConnection.mockReset()
  createConnection.mockImplementation(function () {
    const s = new MockSocket()
    sockets.push(s)
    return s
  })
  return {
    client: new BluezDeviceClient('/tmp/test.sock'),
    nextSocket: () => sockets.shift()!
  }
}

describe('BluezDeviceClient.listPaired', () => {
  test('writes "list_paired" and resolves with the devices array', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('list_paired\n')

    const resp = JSON.stringify({ ok: true, devices: [{ mac: 'AA:BB', name: 'Test' }] }) + '\n'
    sock.emit('data', Buffer.from(resp))

    const devices = await p
    expect(devices).toEqual([{ mac: 'AA:BB', name: 'Test' }])
  })

  test('rejects when the server reports !ok', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: false, error: 'no bt' }) + '\n'))
    await expect(p).rejects.toThrow(BluezDeviceError)
  })
})

describe('BluezDeviceClient.connect / disconnect / remove', () => {
  test('connect writes "connect <mac>"', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.connect('AA:BB', 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('connect AA:BB\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('connectFull writes "connect-full <mac>"', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.connectFull('AA:BB', 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('connect-full AA:BB\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('disconnect writes "disconnect <mac>"', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.disconnect('CC:DD', 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('disconnect CC:DD\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('disconnectProfile writes "disconnect-profile <mac> <uuid>"', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.disconnectProfile('CC:DD', '0000111f-0000-1000-8000-00805f9b34fb', 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith(
      'disconnect-profile CC:DD 0000111f-0000-1000-8000-00805f9b34fb\n'
    )
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('remove writes "remove <mac>"', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.remove('EE:FF', 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('remove EE:FF\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })
})

describe('BluezDeviceClient — failure paths', () => {
  test('rejects on socket error', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('error', new Error('ECONNREFUSED'))
    await expect(p).rejects.toThrow(/ECONNREFUSED/)
  })

  test('rejects on bad JSON', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('data', Buffer.from('not json\n'))
    await expect(p).rejects.toThrow(/bad json/)
  })

  test('rejects when the socket closes without sending a complete line', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('end')
    await expect(p).rejects.toThrow(/closed without response/)
  })

  test('rejects after timeout', async () => {
    vi.useFakeTimers()
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(50)
    nextSocket() // hold the socket but never respond
    vi.advanceTimersByTime(200)
    await expect(p).rejects.toThrow(/timeout/)
    vi.useRealTimers()
  })
})

describe('BluezDeviceClient.subscribe', () => {
  test('writes "subscribe" and forwards each newline-delimited event', () => {
    const { client, nextSocket } = makeClient()
    const events: unknown[] = []
    client.subscribe((ev) => events.push(ev))
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('subscribe\n')

    sock.emit(
      'data',
      Buffer.from(
        JSON.stringify({ event: 'device-connected', mac: 'AA' }) +
          '\n' +
          JSON.stringify({ event: 'device-removed', mac: 'BB' }) +
          '\n'
      )
    )
    expect(events).toEqual([
      { event: 'device-connected', mac: 'AA' },
      { event: 'device-removed', mac: 'BB' }
    ])
  })

  test('ignores malformed lines and missing "event" fields', () => {
    const { client, nextSocket } = makeClient()
    const events: unknown[] = []
    client.subscribe((ev) => events.push(ev))
    const sock = nextSocket()
    sock.emit('connect')

    sock.emit('data', Buffer.from('garbage\n'))
    sock.emit('data', Buffer.from(JSON.stringify({ no_event_field: true }) + '\n'))
    expect(events).toHaveLength(0)
  })

  test('skips empty lines in the event stream', () => {
    const { client, nextSocket } = makeClient()
    const events: unknown[] = []
    client.subscribe((ev) => events.push(ev))
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('data', Buffer.from('\n' + JSON.stringify({ event: 'x' }) + '\n\n'))
    expect(events).toEqual([{ event: 'x' }])
  })

  test('fires onOpen once the subscription socket connects', () => {
    const { client, nextSocket } = makeClient()
    const onOpen = vi.fn()
    client.subscribe(() => {}, undefined, onOpen)
    const sock = nextSocket()
    sock.emit('connect')
    expect(onOpen).toHaveBeenCalledTimes(1)
  })

  test('close without an onClose callback is harmless', () => {
    const { client, nextSocket } = makeClient()
    client.subscribe(() => {})
    const sock = nextSocket()
    expect(() => sock.emit('close')).not.toThrow()
  })

  test('fires onClose on socket close', async () => {
    const { client, nextSocket } = makeClient()
    const onClose = vi.fn()
    client.subscribe(() => {}, onClose)
    const sock = nextSocket()
    sock.emit('close')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  test('fires onClose only once even on close + error', async () => {
    const { client, nextSocket } = makeClient()
    const onClose = vi.fn()
    client.subscribe(() => {}, onClose)
    const sock = nextSocket()
    sock.emit('error', new Error('x'))
    sock.emit('close')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  test('returned close() destroys the socket', async () => {
    const { client, nextSocket } = makeClient()
    const handle = client.subscribe(() => {})
    const sock = nextSocket()
    handle.close()
    expect(sock.destroy).toHaveBeenCalled()
  })
})

describe('BluezDeviceClient — request internals', () => {
  test('chunked data without newline is buffered until the line terminator arrives', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    // Split JSON across multiple chunks; resolve only after \n arrives
    sock.emit('data', Buffer.from('{"ok":'))
    sock.emit('data', Buffer.from('true,"devices":[]}'))
    sock.emit('data', Buffer.from('\n'))
    await expect(p).resolves.toEqual([])
  })

  test('"end" after a complete response does not reject', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true, devices: [] }) + '\n'))
    sock.emit('end')
    await expect(p).resolves.toEqual([])
  })

  test('listPaired throws a fallback error when server omits .error', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: false }) + '\n'))
    await expect(p).rejects.toThrow(/list_paired failed/)
  })

  test.each([
    ['connect', 'connect AA:BB'],
    ['disconnect', 'disconnect AA:BB'],
    ['remove', 'remove AA:BB']
  ])('%s with the default timeout argument still works', async (method, expectedWrite) => {
    const { client, nextSocket } = makeClient()
    type Method = (mac: string) => Promise<unknown>
    const p = ((client as unknown as Record<string, Method>)[method] as Method).call(
      client,
      'AA:BB'
    )
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith(expectedWrite + '\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('connect with a uuid appends it to the command line', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.connect('AA:BB', 500, 'uuid-1234')
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('connect AA:BB uuid-1234\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('setWiredPhones serializes the id list', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.setWiredPhones(['id1', 'id2'], 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('wired-phones ["id1","id2"]\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('setScoSink names the feed and stream, or clears it', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.setScoSink('/tmp/feed.sock', 5, 500)
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('sco-sink /tmp/feed.sock 5\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })

    const p2 = client.setScoSink(undefined, undefined, 500)
    const sock2 = nextSocket()
    sock2.emit('connect')
    expect(sock2.write).toHaveBeenCalledWith('sco-sink\n')
    sock2.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p2).resolves.toEqual({ ok: true })
  })

  test('deauthApClients with the default timeout writes deauth-ap', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.deauthApClients()
    const sock = nextSocket()
    sock.emit('connect')
    expect(sock.write).toHaveBeenCalledWith('deauth-ap\n')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true }) + '\n'))
    await expect(p).resolves.toEqual({ ok: true })
  })

  test('a late socket error after settling is ignored', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.emit('connect')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true, devices: [] }) + '\n'))
    sock.emit('error', new Error('late'))
    await expect(p).resolves.toEqual([])
  })

  test('socket.destroy throwing is swallowed', async () => {
    const { client, nextSocket } = makeClient()
    const p = client.listPaired(1000)
    const sock = nextSocket()
    sock.destroy = vi.fn(function () {
      throw new Error('already gone')
    })
    sock.emit('connect')
    sock.emit('data', Buffer.from(JSON.stringify({ ok: true, devices: [] }) + '\n'))
    await expect(p).resolves.toEqual([])
  })
})
