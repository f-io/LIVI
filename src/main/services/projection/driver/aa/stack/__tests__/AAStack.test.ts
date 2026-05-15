import { EventEmitter } from 'node:events'

class MockSession extends EventEmitter {
  sendTouch = jest.fn()
  sendButton = jest.fn()
  sendRotary = jest.fn()
  sendFuelData = jest.fn()
  sendSpeedData = jest.fn()
  sendRpmData = jest.fn()
  sendGearData = jest.fn()
  sendNightModeData = jest.fn()
  sendParkingBrakeData = jest.fn()
  sendLightData = jest.fn()
  sendEnvironmentData = jest.fn()
  sendOdometerData = jest.fn()
  sendDrivingStatusData = jest.fn()
  sendGpsLocationData = jest.fn()
  sendVehicleEnergyModel = jest.fn()
  sendMicPcm = jest.fn()
  requestKeyframe = jest.fn()
  requestClusterKeyframe = jest.fn()
  requestShutdown = jest.fn(async () => undefined)
  start = jest.fn(async () => undefined)
  close = jest.fn()
}

class MockTcpServer extends EventEmitter {
  listen = jest.fn()
  close = jest.fn()
}

jest.mock('../session/Session', () => ({
  Session: jest.fn().mockImplementation(() => new MockSession())
}))

jest.mock('../transport/TcpServer', () => ({
  TcpServer: jest.fn().mockImplementation(() => new MockTcpServer())
}))

jest.mock('../system/hwaddr', () => ({
  detectBtMac: jest.fn(() => 'AA:BB:CC:DD:EE:FF'),
  detectWifiBssid: jest.fn(() => '11:22:33:44:55:66')
}))

import type * as net from 'node:net'
import { AAStack, type AAStackConfig } from '../index'

beforeEach(() => {
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
  ;(jest.requireMock('../session/Session') as { Session: jest.Mock }).Session.mockReset()
  ;(jest.requireMock('../session/Session') as { Session: jest.Mock }).Session.mockImplementation(
    () => new MockSession()
  )
  ;(jest.requireMock('../transport/TcpServer') as { TcpServer: jest.Mock }).TcpServer.mockReset()
  ;(
    jest.requireMock('../transport/TcpServer') as { TcpServer: jest.Mock }
  ).TcpServer.mockImplementation(() => new MockTcpServer())
})
afterEach(() => jest.restoreAllMocks())

function baseCfg(over: Partial<AAStackConfig> = {}): AAStackConfig {
  return {
    huName: 'LIVI',
    clusterWidth: 0,
    clusterHeight: 0,
    clusterFps: 0,
    clusterDpi: 0,
    ...over
  } as AAStackConfig
}

function setup() {
  const stack = new AAStack(baseCfg())
  const server = (stack as unknown as { _server: MockTcpServer })._server
  // Drive a session through the server
  const session = new MockSession()
  server.emit('session', session)
  return { stack, server, session }
}

describe('AAStack — construction', () => {
  test('auto-detects btMacAddress + wifiBssid when missing', () => {
    const cfg = baseCfg()
    new AAStack(cfg)
    expect(cfg.btMacAddress).toBe('AA:BB:CC:DD:EE:FF')
    expect(cfg.wifiBssid).toBe('11:22:33:44:55:66')
  })

  test('skips auto-detection when provided', () => {
    const cfg = baseCfg({ btMacAddress: 'preset', wifiBssid: 'wlan-mac' })
    new AAStack(cfg)
    expect(cfg.btMacAddress).toBe('preset')
    expect(cfg.wifiBssid).toBe('wlan-mac')
  })
})

describe('AAStack — lifecycle', () => {
  test('start() listens on the configured port', () => {
    const stack = new AAStack(baseCfg({ port: 5277 }))
    const server = (stack as unknown as { _server: MockTcpServer })._server
    stack.start()
    expect(server.listen).toHaveBeenCalledWith(5277)
  })

  test('stop() closes the active session and the server', () => {
    const { stack, server, session } = setup()
    stack.stop()
    expect(session.close).toHaveBeenCalled()
    expect(server.close).toHaveBeenCalled()
  })

  test('stop() without an active session still closes the server', () => {
    const stack = new AAStack(baseCfg())
    const server = (stack as unknown as { _server: MockTcpServer })._server
    stack.stop()
    expect(server.close).toHaveBeenCalled()
  })

  test('session.close throwing is swallowed during stop()', () => {
    const { stack, session } = setup()
    session.close.mockImplementation(() => {
      throw new Error('already closed')
    })
    expect(() => stack.stop()).not.toThrow()
  })
})

describe('AAStack — event forwarding', () => {
  test('forwards video / audio / nav events from the active session', () => {
    const { session, stack } = setup()
    const events: string[] = []
    const expected = [
      'video-frame',
      'cluster-video-frame',
      'video-codec',
      'cluster-video-codec',
      'audio-frame',
      'audio-start',
      'audio-stop',
      'mic-start',
      'mic-stop',
      'voice-session',
      'host-ui-requested',
      'video-focus-projected',
      'cluster-video-focus-projected',
      'media-metadata',
      'media-status',
      'nav-start',
      'nav-stop',
      'nav-status',
      'nav-turn',
      'nav-distance',
      'connected',
      'disconnected'
    ]
    for (const e of expected) stack.on(e, () => events.push(e))
    for (const e of expected) session.emit(e)
    expect(events).toEqual(expected)
  })

  test('session "error" is forwarded', () => {
    const { session, stack } = setup()
    const onError = jest.fn()
    stack.on('error', onError)
    session.emit('error', new Error('x'))
    expect(onError).toHaveBeenCalled()
  })

  test('server "error" is forwarded', () => {
    const stack = new AAStack(baseCfg())
    const server = (stack as unknown as { _server: MockTcpServer })._server
    const onError = jest.fn()
    stack.on('error', onError)
    server.emit('error', new Error('eaddrinuse'))
    expect(onError).toHaveBeenCalled()
  })
})

describe('AAStack — outbound API delegates to active session', () => {
  test('without an active session, calls are silently dropped', () => {
    const stack = new AAStack(baseCfg())
    expect(() => {
      stack.sendTouch(0, [{ x: 0, y: 0, id: 0 }])
      stack.sendButton(3, true)
      stack.sendRotary(1)
      stack.sendFuelData(50)
      stack.sendSpeedData(10_000)
      stack.sendRpmData(2_000_000)
      stack.sendGearData(4)
      stack.sendNightModeData(true)
      stack.sendParkingBrakeData(false)
      stack.sendLightData(1, false, 2)
      stack.sendEnvironmentData(20_000)
      stack.sendOdometerData(120_000)
      stack.sendDrivingStatusData(0)
      stack.sendGpsLocationData({ latDeg: 52, lngDeg: 13 })
      stack.sendVehicleEnergyModel(50_000, 30_000, 200_000)
      stack.sendMicPcm(Buffer.alloc(0))
      stack.requestKeyframe()
      stack.requestClusterKeyframe()
    }).not.toThrow()
  })

  test('every outbound method delegates to the active session', async () => {
    const { stack, session } = setup()
    stack.sendTouch(0, [{ x: 0, y: 0, id: 0 }], 0)
    stack.sendButton(3, true)
    stack.sendRotary(1)
    stack.sendFuelData(50, 200, true)
    stack.sendSpeedData(10_000, true, 12_000)
    stack.sendRpmData(2_000_000)
    stack.sendGearData(4)
    stack.sendNightModeData(true)
    stack.sendParkingBrakeData(false)
    stack.sendLightData(1, false, 2)
    stack.sendEnvironmentData(20_000, 101_000, 0)
    stack.sendOdometerData(120_000)
    stack.sendDrivingStatusData(0)
    stack.sendGpsLocationData({ latDeg: 52, lngDeg: 13 })
    stack.sendVehicleEnergyModel(50_000, 30_000, 200_000)
    stack.sendMicPcm(Buffer.from([1]))
    stack.requestKeyframe()
    stack.requestClusterKeyframe()
    await stack.requestShutdown()

    expect(session.sendTouch).toHaveBeenCalled()
    expect(session.sendButton).toHaveBeenCalled()
    expect(session.sendRotary).toHaveBeenCalled()
    expect(session.sendFuelData).toHaveBeenCalled()
    expect(session.sendSpeedData).toHaveBeenCalled()
    expect(session.sendRpmData).toHaveBeenCalled()
    expect(session.sendGearData).toHaveBeenCalled()
    expect(session.sendNightModeData).toHaveBeenCalled()
    expect(session.sendParkingBrakeData).toHaveBeenCalled()
    expect(session.sendLightData).toHaveBeenCalled()
    expect(session.sendEnvironmentData).toHaveBeenCalled()
    expect(session.sendOdometerData).toHaveBeenCalled()
    expect(session.sendDrivingStatusData).toHaveBeenCalled()
    expect(session.sendGpsLocationData).toHaveBeenCalled()
    expect(session.sendVehicleEnergyModel).toHaveBeenCalled()
    expect(session.sendMicPcm).toHaveBeenCalled()
    expect(session.requestKeyframe).toHaveBeenCalled()
    expect(session.requestClusterKeyframe).toHaveBeenCalled()
    expect(session.requestShutdown).toHaveBeenCalled()
  })
})

describe('AAStack.attachSocket', () => {
  test('constructs a Session and starts it', () => {
    const stack = new AAStack(baseCfg())
    const sock = { setNoDelay: jest.fn() } as unknown as net.Socket
    const session = stack.attachSocket(sock)
    expect((sock as unknown as { setNoDelay: jest.Mock }).setNoDelay).toHaveBeenCalledWith(true)
    expect(session).toBeDefined()
    expect((session as unknown as MockSession).start).toHaveBeenCalled()
  })

  test('attachSocket session "error" + "disconnected" are logged with the loopback tag', () => {
    const errLog = jest.spyOn(console, 'error').mockImplementation(() => {})
    const log = jest.spyOn(console, 'log').mockImplementation(() => {})
    const stack = new AAStack(baseCfg())
    stack.on('error', () => {})
    const sock = { setNoDelay: jest.fn() } as unknown as net.Socket
    const session = stack.attachSocket(sock) as unknown as MockSession
    session.emit('error', new Error('reset'))
    session.emit('disconnected', 'phone closed')
    session.emit('disconnected') // no reason → '' fallback
    expect(errLog).toHaveBeenCalled()
    expect(log).toHaveBeenCalled()
    errLog.mockRestore()
    log.mockRestore()
  })

  test('attachSocket session.start rejecting is caught and logged', async () => {
    const errLog = jest.spyOn(console, 'error').mockImplementation(() => {})
    const { Session } = jest.requireMock('../session/Session') as { Session: jest.Mock }
    Session.mockImplementationOnce(() => {
      const s = new MockSession()
      s.start = jest.fn(async () => {
        throw new Error('TLS rejected')
      })
      return s
    })
    const stack = new AAStack(baseCfg())
    const sock = { setNoDelay: jest.fn() } as unknown as net.Socket
    stack.attachSocket(sock)
    await new Promise((r) => setImmediate(r))
    expect(errLog).toHaveBeenCalledWith(expect.stringContaining('start error'), 'TLS rejected')
    errLog.mockRestore()
  })

  test('attachSocket session.disconnected with no reason yields empty-string log', () => {
    const log = jest.spyOn(console, 'log').mockImplementation(() => {})
    const stack = new AAStack(baseCfg())
    const sock = { setNoDelay: jest.fn() } as unknown as net.Socket
    const session = stack.attachSocket(sock) as unknown as MockSession
    session.emit('disconnected', undefined)
    expect(log).toHaveBeenCalled()
    log.mockRestore()
  })
})
