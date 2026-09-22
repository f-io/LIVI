import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

class MockSession extends EventEmitter {
  sendTouch = vi.fn()
  sendButton = vi.fn()
  sendRotary = vi.fn()
  sendFuelData = vi.fn()
  sendSpeedData = vi.fn()
  sendRpmData = vi.fn()
  sendGearData = vi.fn()
  sendNightModeData = vi.fn()
  sendParkingBrakeData = vi.fn()
  sendLightData = vi.fn()
  sendEnvironmentData = vi.fn()
  sendOdometerData = vi.fn()
  sendDrivingStatusData = vi.fn()
  sendGpsLocationData = vi.fn()
  sendVehicleEnergyModel = vi.fn()
  micFormat = vi.fn(() => ({ sampleRate: 24000, channels: 2 }))
  micSocketPath = vi.fn((): string | null => '/run/livi/aa-mic.sock')
  requestVideoFocus = vi.fn()
  requestMainKeyframe = vi.fn()
  requestClusterKeyframe = vi.fn()
  forceClusterKeyframe = vi.fn()
  setClusterStreamActive = vi.fn()
  requestShutdown = vi.fn(async () => undefined)
  sendMediaSink = vi.fn()
  start = vi.fn(async () => undefined)
  close = vi.fn()
}

vi.mock('../session/Session', () => ({
  Session: vi.fn().mockImplementation(function () {
    return new MockSession()
  })
}))

vi.mock('../system/hwaddr', () => ({
  detectBtMac: vi.fn(() => 'AA:BB:CC:DD:EE:FF'),
  detectWifiBssid: vi.fn(() => '11:22:33:44:55:66')
}))

import { AAStack, type AAStackConfig } from '../index'
import type { HelperSessionLink } from '../transport/HelperSessionLink'

/** The helper's session link, as far as the mocked session needs it. */
class FakeLink extends EventEmitter {
  readonly peer = '10.0.0.2'
  closed = false
  send = vi.fn()
  control = vi.fn()
  end = vi.fn()
  destroy = vi.fn()
}

const fakeLink = (): HelperSessionLink => new FakeLink() as unknown as HelperSessionLink

beforeEach(async () => {
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  vi.spyOn(console, 'error').mockImplementation(function () {})
  ;((await vi.importMock('../session/Session')) as { Session: Mock }).Session.mockReset()
  ;((await vi.importMock('../session/Session')) as { Session: Mock }).Session.mockImplementation(
    function () {
      return new MockSession()
    }
  )
})
afterEach(async () => vi.restoreAllMocks())

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
  const link = fakeLink()
  const session = stack.attachLink(link) as unknown as MockSession
  return { stack, session, link }
}

describe('AAStack: construction', () => {
  test('auto-detects btMacAddress + wifiBssid when missing', async () => {
    const cfg = baseCfg()
    new AAStack(cfg)
    expect(cfg.btMacAddress).toBe('AA:BB:CC:DD:EE:FF')
    expect(cfg.wifiBssid).toBe('11:22:33:44:55:66')
  })

  test('skips auto-detection when provided', async () => {
    const cfg = baseCfg({ btMacAddress: 'preset', wifiBssid: 'wlan-mac' })
    new AAStack(cfg)
    expect(cfg.btMacAddress).toBe('preset')
    expect(cfg.wifiBssid).toBe('wlan-mac')
  })
})

describe('AAStack: lifecycle', () => {
  test('stop() closes the active session', async () => {
    const { stack, session } = setup()
    stack.stop()
    expect(session.close).toHaveBeenCalled()
    expect(stack.activeSession).toBeNull()
  })

  test('stop() without an active session does not throw', async () => {
    const stack = new AAStack(baseCfg())
    expect(() => stack.stop()).not.toThrow()
  })

  test('session.close throwing is swallowed during stop()', async () => {
    const { stack, session } = setup()
    session.close.mockImplementation(function () {
      throw new Error('already closed')
    })
    expect(() => stack.stop()).not.toThrow()
  })
})

describe('AAStack: event forwarding', () => {
  test('forwards codec / audio / nav events from the active session', async () => {
    const { session, stack } = setup()
    const events: string[] = []
    const expected = [
      'video-codec',
      'cluster-video-codec',
      'audio-setup',
      'audio-start',
      'audio-stop',
      'mic-start',
      'mic-stop',
      'voice-session',
      'audio-focus',
      'host-ui-requested',
      'device-info',
      'device-status',
      'video-focus-projected',
      'cluster-video-focus-projected',
      'video-started',
      'cluster-video-started',
      'media-metadata',
      'media-status',
      'nav-start',
      'nav-stop',
      'nav-status',
      'nav-turn',
      'nav-distance',
      'nav-state',
      'nav-position',
      'connected',
      'disconnected'
    ]
    for (const e of expected) stack.on(e, () => events.push(e))
    for (const e of expected) session.emit(e)
    expect(events).toEqual(expected)
  })

  test('the media itself never passes through the stack', () => {
    const { session, stack } = setup()
    const seen = vi.fn()
    for (const e of ['video-frame', 'cluster-video-frame', 'audio-frame']) stack.on(e, seen)
    session.emit('video-frame', Buffer.alloc(1), 0n)
    session.emit('audio-frame', Buffer.alloc(1), 0n, 'media', 4)
    expect(seen).not.toHaveBeenCalled()
  })

  test('session "error" is forwarded', () => {
    const { session, stack } = setup()
    const onError = vi.fn()
    stack.on('error', onError)
    session.emit('error', new Error('x'))
    expect(onError).toHaveBeenCalled()
  })
})

describe('AAStack: mic format', () => {
  test('falls back to 16 kHz mono without an active session', () => {
    const stack = new AAStack(baseCfg())
    expect(stack.micFormat()).toEqual({ sampleRate: 16000, channels: 1 })
  })

  test('reports the active session format', () => {
    const { stack, session } = setup()
    expect(stack.micFormat()).toEqual({ sampleRate: 24000, channels: 2 })
    expect(session.micFormat).toHaveBeenCalled()
  })
})

describe('AAStack: mic socket', () => {
  test('has no socket without an active session', () => {
    const stack = new AAStack(baseCfg())
    expect(stack.micSocketPath()).toBeNull()
  })

  test('delegates micSocketPath to the active session', () => {
    const { stack, session } = setup()
    expect(stack.micSocketPath()).toBe('/run/livi/aa-mic.sock')
    expect(session.micSocketPath).toHaveBeenCalledTimes(1)
    session.micSocketPath.mockReturnValue(null)
    expect(stack.micSocketPath()).toBeNull()
  })
})

describe('AAStack: outbound API delegates to active session', () => {
  test('without an active session, calls are silently dropped', async () => {
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
      stack.requestVideoFocus()
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
    stack.requestVideoFocus()
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
    expect(session.requestVideoFocus).toHaveBeenCalled()
    expect(session.requestClusterKeyframe).toHaveBeenCalled()
    expect(session.requestShutdown).toHaveBeenCalled()
  })
})

describe('AAStack: config + keyframe API', () => {
  test('applyDisplayConfig merges into the stored config', () => {
    const cfg = baseCfg()
    const stack = new AAStack(cfg)
    stack.applyDisplayConfig(baseCfg({ videoWidth: 1920 }) as AAStackConfig)
    expect((stack as unknown as { _cfg: AAStackConfig })._cfg.videoWidth).toBe(1920)
  })

  test('setConfigRefresh runs before a session is attached', () => {
    const stack = new AAStack(baseCfg())
    const fn = vi.fn()
    stack.setConfigRefresh(fn)
    stack.attachLink(fakeLink())
    expect(fn).toHaveBeenCalledTimes(1)
  })

  test('activeSession getter reflects the adopted session', () => {
    const stack = new AAStack(baseCfg())
    expect(stack.activeSession).toBeNull()
    const { stack: s2, session } = setup()
    expect(s2.activeSession).toBe(session as never)
  })

  test('keyframe + cluster-stream methods delegate to the active session', () => {
    const { stack, session } = setup()
    stack.requestMainKeyframe()
    stack.forceClusterKeyframe()
    stack.setClusterStreamActive(false)
    expect(session.requestMainKeyframe).toHaveBeenCalled()
    expect(session.forceClusterKeyframe).toHaveBeenCalled()
    expect(session.setClusterStreamActive).toHaveBeenCalledWith(false)
  })

  test('keyframe methods are safe with no active session', () => {
    const stack = new AAStack(baseCfg())
    expect(() => {
      stack.requestMainKeyframe()
      stack.forceClusterKeyframe()
      stack.setClusterStreamActive(true)
    }).not.toThrow()
  })
})

describe('AAStack.attachLink', () => {
  test('constructs a Session on the link and starts it', async () => {
    const stack = new AAStack(baseCfg())
    const link = fakeLink()
    const { Session } = (await vi.importMock('../session/Session')) as { Session: Mock }
    const session = stack.attachLink(link)
    expect(Session).toHaveBeenCalledWith(link, expect.anything())
    expect((session as unknown as MockSession).start).toHaveBeenCalled()
  })

  test('sendMediaSink forwards to the active session and is a no-op without one', () => {
    const stack = new AAStack(baseCfg())
    expect(() => stack.sendMediaSink({ feed: '/tmp/f' })).not.toThrow()
    const session = stack.attachLink(fakeLink()) as unknown as MockSession
    stack.sendMediaSink({ feed: '/tmp/f', video: [] })
    expect(session.sendMediaSink).toHaveBeenCalledWith({ feed: '/tmp/f', video: [] })
  })

  test('session "error" + "disconnected" are logged with the helper peer', () => {
    const errLog = vi.spyOn(console, 'error').mockImplementation(function () {})
    const log = vi.spyOn(console, 'log').mockImplementation(function () {})
    const stack = new AAStack(baseCfg())
    stack.on('error', () => {})
    const session = stack.attachLink(fakeLink()) as unknown as MockSession
    session.emit('error', new Error('reset'))
    session.emit('disconnected', 'phone closed')
    session.emit('disconnected')
    expect(errLog).toHaveBeenCalledWith(expect.stringContaining('helper 10.0.0.2'), 'reset')
    expect(log).toHaveBeenCalledWith(expect.stringContaining('helper 10.0.0.2'))
    errLog.mockRestore()
    log.mockRestore()
  })

  test('session.start rejecting is caught and logged', async () => {
    const errLog = vi.spyOn(console, 'error').mockImplementation(function () {})
    const { Session } = (await vi.importMock('../session/Session')) as { Session: Mock }
    Session.mockImplementationOnce(function () {
      const s = new MockSession()
      s.start = vi.fn(async () => {
        throw new Error('setup rejected')
      })
      return s
    })
    const stack = new AAStack(baseCfg())
    stack.attachLink(fakeLink())
    await new Promise((r) => setImmediate(r))
    expect(errLog).toHaveBeenCalledWith(expect.stringContaining('start error'), 'setup rejected')
    errLog.mockRestore()
  })
})
