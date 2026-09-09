import { EventEmitter } from 'node:events'
import type { Config } from '@shared/types/Config'

class MockSocket extends EventEmitter {
  sent: string[] = []
  destroyed = false
  write(text: string): boolean {
    this.sent.push(text)
    return true
  }
  destroy(): void {
    this.destroyed = true
  }
  /** The dongle answering, one chunk as it would arrive. */
  say(text: string): void {
    this.emit('data', Buffer.from(text))
  }
}

const { sockets, createConnection } = vi.hoisted(() => {
  const list: MockSocket[] = []
  return {
    sockets: list,
    createConnection: vi.fn(() => {
      const s = new MockSocket()
      list.push(s)
      // The caller writes on connect, so the event has to land after it subscribed.
      queueMicrotask(() => s.emit('connect'))
      return s
    })
  }
})

vi.mock('node:net', () => ({ default: { createConnection }, createConnection }))

const { networkInterfaces } = vi.hoisted(() => ({
  networkInterfaces: vi.fn(
    (): Record<string, { address: string }[]> => ({
      ncm0: [{ address: '10.10.10.100' }]
    })
  )
}))

vi.mock('node:os', () => ({ default: { networkInterfaces }, networkInterfaces }))

import {
  btCommandsFor,
  commandsFor,
  DONGLE_LINK,
  dongleApMac,
  dongleApPresent,
  reconcileDongleAp,
  releaseDongle
} from '../dongleAp'

const config = {
  wifiInterface: 'wlan0',
  carName: 'Volvo',
  country: 'DE',
  wifiChannel: 44,
  wifiPassword: 'geheim12'
} as Config

beforeEach(() => {
  sockets.length = 0
  createConnection.mockClear()
  networkInterfaces.mockReturnValue({ ncm0: [{ address: '10.10.10.100' }] })
})

/** Waits until the next connection has been opened. */
async function settle(count: number): Promise<void> {
  for (let i = 0; i < 200 && sockets.length <= count; i++) await Promise.resolve()
}

/** Waits for the command to go out, then answers it. */
async function answer(socket: MockSocket, count: number, text = 'ok\n'): Promise<void> {
  for (let i = 0; i < 200 && socket.sent.length < count; i++) await Promise.resolve()
  socket.say(text)
}

describe('what the dongle is told', () => {
  it('silences it while something else is the access point', () => {
    expect(commandsFor(config)).toEqual(['off'])
  })

  it('follows the bluetooth setting, not the wifi one', () => {
    expect(btCommandsFor(config)).toEqual(['off'])
    const chosen = {
      ...config,
      btAdapter: DONGLE_LINK,
      wirelessCpEnabled: true,
      autoConn: true
    } as Config
    expect(btCommandsFor(chosen)).toEqual(['on', 'reconnect on'])
    expect(btCommandsFor({ ...chosen, autoConn: false } as Config)).toEqual(['on', 'reconnect off'])
  })

  it('silences the accessory when wireless CarPlay is off', () => {
    const chosen = { ...config, btAdapter: DONGLE_LINK, wirelessCpEnabled: false } as Config
    expect(btCommandsFor(chosen)).toEqual(['off'])
  })

  it('hands over the settings once it is the access point', () => {
    expect(commandsFor({ ...config, wifiInterface: DONGLE_LINK })).toEqual([
      'set ssid Volvo',
      'set country DE',
      'set channel 44',
      'set passphrase geheim12',
      'apply',
      'save'
    ])
  })

  it('stands in for a setting that was left empty', () => {
    const bare = { ...config, wifiInterface: DONGLE_LINK, carName: '', wifiPassword: '' } as Config
    expect(commandsFor(bare)).toContain('set ssid LIVI')
    expect(commandsFor(bare)).toContain('set passphrase 12345678')
  })
})

describe('talking to the dongle', () => {
  it('sends the next command only after the one before was taken', async () => {
    const done = reconcileDongleAp(config)
    const socket = sockets[0]
    await answer(socket, 1)
    await answer(socket, 2, 'mac 02:50:43:02:ff:01\nok\n')
    await settle(1)
    await answer(sockets[1], 1)
    await done
    expect(socket.sent).toEqual(['off\n', 'status\n'])
    expect(sockets[1].sent).toEqual(['off\n'])
    expect(socket.destroyed).toBe(true)
  })

  it('remembers the access point MAC the state carries', async () => {
    const done = reconcileDongleAp(config)
    const socket = sockets[0]
    await answer(socket, 1)
    await answer(socket, 2, 'state on\nmac 02:50:43:02:ff:01\nok\n')
    await settle(1)
    await answer(sockets[1], 1)
    await done
    expect(dongleApMac()).toBe('02:50:43:02:ff:01')
  })

  it('gives up on a refusal instead of carrying on', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const done = reconcileDongleAp({ ...config, wifiInterface: DONGLE_LINK })
    const socket = sockets[0]
    await answer(socket, 1, 'error channel is out of range\n')
    await settle(1)
    await answer(sockets[1], 1)
    await done
    expect(socket.sent).toEqual(['set ssid Volvo\n'])
    expect(warn.mock.calls[0]?.[1]).toContain('channel is out of range')
    warn.mockRestore()
  })

  it('reads the lines an answer carries before its ok', async () => {
    const probe = dongleApPresent()
    const socket = sockets[0]
    await answer(socket, 1, 'state on\nbt off\nok\n')
    expect(await probe).toBe(true)
  })

  it('reports no dongle when the link fails', async () => {
    const probe = dongleApPresent()
    for (let i = 0; i < 50 && sockets.length === 0; i++) await Promise.resolve()
    sockets[0].emit('error', new Error('ENOTFOUND'))
    expect(await probe).toBe(false)
  })

  it('says nothing at all while no dongle is plugged in', async () => {
    networkInterfaces.mockReturnValue({ wlan0: [{ address: '192.168.1.20' }] })
    await reconcileDongleAp(config)
    await releaseDongle()
    expect(await dongleApPresent()).toBe(false)
    expect(createConnection).not.toHaveBeenCalled()
  })
})
