import { EventEmitter } from 'node:events'

class MockAAStack extends EventEmitter {
  cfg: unknown
  start = vi.fn()
  stop = vi.fn()
  attachSocket = vi.fn()
  setConfigRefresh = vi.fn()
  setClusterStreamActive = vi.fn()
  applyDisplayConfig = vi.fn()
  requestVideoFocus = vi.fn()
  requestMainKeyframe = vi.fn()
  requestClusterKeyframe = vi.fn()
  forceClusterKeyframe = vi.fn()
  requestShutdown = vi.fn(async () => undefined)
  sendTouch = vi.fn()
  sendButton = vi.fn()
  sendRotary = vi.fn()
  sendMicPcm = vi.fn()
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
  constructor(cfg: unknown) {
    super()
    this.cfg = cfg
  }
}

class MockUsbAoapBridge extends EventEmitter {
  start = vi.fn(async () => undefined)
  stop = vi.fn(async () => undefined)
  drain = vi.fn(async () => undefined)
  forceReenum = vi.fn(async () => undefined)
}

class MockMicrophone extends EventEmitter {
  start = vi.fn()
  stop = vi.fn()
}

class MockSocket extends EventEmitter {
  destroy = vi.fn()
}

const lastAaStack: { instance: MockAAStack | null } = { instance: null }

vi.mock('../stack/index', async () => {
  const real = await vi.importActual('../stack/index')
  return {
    ...real,
    AAStack: vi.fn().mockImplementation(function (cfg: unknown) {
      const aa = new MockAAStack(cfg)
      lastAaStack.instance = aa
      return aa
    })
  }
})

vi.mock('@main/services/audio', () => ({
  Microphone: vi.fn().mockImplementation(function () {
    return new MockMicrophone()
  })
}))

import type * as net from 'node:net'
import type { Config } from '@shared/types'
import { CommandMapping, MultiTouchAction, TouchAction } from '@shared/types/ProjectionEnums'
import {
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone,
  SendMultiTouch,
  SendTouch
} from '../../../messages/sendable'
import { AaSession, type AaSessionSeed } from '../AaSession'
import type { UsbAoapBridge } from '../stack/transport/UsbAoapBridge'

const baseCfg = (): Config =>
  ({
    projectionWidth: 1280,
    projectionHeight: 720,
    projectionFps: 30,
    projectionDpi: 0,
    hand: 0,
    format: 0,
    iBoxVersion: 0,
    phoneWorkMode: 0,
    packetMax: 0,
    boxName: 'LIVI',
    carName: 'LIVI',
    wifiPassword: 'pw',
    wifiChannel: 36,
    clusterWidth: 800,
    clusterHeight: 480,
    clusterFps: 30,
    clusterDpi: 0,
    projectionSafeAreaTop: 0,
    projectionSafeAreaBottom: 0,
    projectionSafeAreaLeft: 0,
    projectionSafeAreaRight: 0,
    clusterSafeAreaTop: 0,
    clusterSafeAreaBottom: 0,
    clusterSafeAreaLeft: 0,
    clusterSafeAreaRight: 0,
    cluster: { main: true, dash: false, aux: false },
    disableAudioOutput: false
  }) as unknown as Config

const baseSeed = (over: Partial<AaSessionSeed> = {}): AaSessionSeed => ({
  hevcSupported: false,
  vp9Supported: false,
  av1Supported: false,
  initialNightMode: undefined,
  clusterStreamActive: true,
  ...over
})

function makeSession(
  opts: {
    cfg?: Config
    wired?: boolean
    wiredBridge?: MockUsbAoapBridge | null
    seed?: AaSessionSeed
  } = {}
): AaSession {
  const cfg = opts.cfg ?? baseCfg()
  return new AaSession({
    socket: new MockSocket() as unknown as net.Socket,
    getConfig: () => cfg,
    wired: opts.wired ?? false,
    wiredBridge: (opts.wiredBridge ?? null) as unknown as UsbAoapBridge | null,
    seed: opts.seed ?? baseSeed()
  })
}

beforeEach(() => {
  lastAaStack.instance = null
  vi.clearAllMocks()
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  vi.spyOn(console, 'error').mockImplementation(function () {})
})
afterEach(() => {
  vi.restoreAllMocks()
})

describe('AaSession — construction', () => {
  test('constructs an AAStack and adopts the socket via attachSocket', () => {
    const sock = new MockSocket() as unknown as net.Socket
    const d = new AaSession({
      socket: sock,
      getConfig: () => baseCfg(),
      wired: false,
      seed: baseSeed()
    })
    expect(d).toBeInstanceOf(AaSession)
    expect(lastAaStack.instance).not.toBeNull()
    expect(lastAaStack.instance!.attachSocket).toHaveBeenCalledWith(sock)
    // Per-connection: it must NOT open its own :5277 listener.
    expect(lastAaStack.instance!.start).not.toHaveBeenCalled()
  })

  test('config seed: hevc/vp9/av1/nightMode are propagated to AAStackConfig', () => {
    makeSession({
      seed: baseSeed({
        hevcSupported: true,
        vp9Supported: true,
        av1Supported: true,
        initialNightMode: true
      })
    })
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.hevcSupported).toBe(true)
    expect(cfg.vp9Supported).toBe(true)
    expect(cfg.av1Supported).toBe(true)
    expect(cfg.initialNightMode).toBe(true)
  })

  test('config seed: clusterStreamActive is applied to the stack', () => {
    makeSession({ seed: baseSeed({ clusterStreamActive: false }) })
    expect(lastAaStack.instance!.setClusterStreamActive).toHaveBeenCalledWith(false)
  })

  test('config: projectionFps=60 → videoFps=60', () => {
    makeSession({ cfg: { ...baseCfg(), projectionFps: 60 } as Config })
    expect((lastAaStack.instance!.cfg as Record<string, unknown>).videoFps).toBe(60)
  })

  test('config: hand=1 → driverPosition=1', () => {
    makeSession({ cfg: { ...baseCfg(), hand: 1 } as Config })
    expect((lastAaStack.instance!.cfg as Record<string, unknown>).driverPosition).toBe(1)
  })

  test('config: empty carName falls back to "LIVI"', () => {
    makeSession({ cfg: { ...baseCfg(), carName: '   ' } as unknown as Config })
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.huName).toBe('LIVI')
    expect(cfg.wifiSsid).toBe('LIVI')
  })

  test('config: wifiPassword defaults to "12345678" when empty', () => {
    makeSession({ cfg: { ...baseCfg(), wifiPassword: '' } as Config })
    expect((lastAaStack.instance!.cfg as Record<string, unknown>).wifiPassword).toBe('12345678')
  })

  test('isWiredMode reflects the ctor flag', () => {
    expect(makeSession({ wired: false }).isWiredMode()).toBe(false)
    expect(makeSession({ wired: true, wiredBridge: new MockUsbAoapBridge() }).isWiredMode()).toBe(
      true
    )
  })

  test('start() is a no-op that never opens a listener', async () => {
    const d = makeSession()
    lastAaStack.instance!.start.mockClear()
    await expect(d.start(baseCfg())).resolves.toBe(true)
    expect(lastAaStack.instance!.start).not.toHaveBeenCalled()
  })
})

describe('AaSession.close', () => {
  test('stops mic + AAStack, drains the wired bridge and destroys the socket', async () => {
    const bridge = new MockUsbAoapBridge()
    const d = makeSession({ wired: true, wiredBridge: bridge })

    await d.close()
    expect(lastAaStack.instance!.requestShutdown).toHaveBeenCalled()
    expect(lastAaStack.instance!.stop).toHaveBeenCalled()
    expect(bridge.drain).toHaveBeenCalled()
    expect(bridge.stop).toHaveBeenCalled()
  })

  test('idempotent — second close is a no-op', async () => {
    const d = makeSession()
    await d.close()
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('emits "disconnected" once when the session was up', async () => {
    const d = makeSession()
    // Simulate the stack reaching a connected state.
    lastAaStack.instance!.emit('connected')
    const cb = vi.fn()
    d.on('disconnected', cb)
    await d.close()
    expect(cb).toHaveBeenCalledTimes(1)
  })

  test('closing a never-connected session still signals "disconnected" (frees its owner)', async () => {
    const d = makeSession({ wired: true, wiredBridge: new MockUsbAoapBridge() })
    const cb = vi.fn()
    d.on('disconnected', cb)
    await d.close()
    expect(cb).toHaveBeenCalledTimes(1)
  })

  test('a natural stack drop before connect signals "disconnected" exactly once', async () => {
    const d = makeSession()
    const cb = vi.fn()
    d.on('disconnected', cb)
    // Stack drops mid-handshake (never emitted "connected").
    lastAaStack.instance!.emit('disconnected', 'pre-RUNNING watchdog')
    await d.close()
    expect(cb).toHaveBeenCalledTimes(1)
  })
})

describe('AaSession.send — bail-out', () => {
  test('returns false after close (no AAStack)', async () => {
    const d = makeSession()
    await d.close()
    const ok = await d.send(new SendCommand('frame'))
    expect(ok).toBe(false)
  })
})

describe('AaSession.send — SendCommand', () => {
  let d: AaSession
  let aa: MockAAStack
  beforeEach(() => {
    d = makeSession()
    aa = lastAaStack.instance!
  })

  test('frame triggers a single requestVideoFocus (VIDEO_FOCUS_REQUEST)', async () => {
    await d.send(new SendCommand('frame'))
    expect(aa.requestVideoFocus).toHaveBeenCalledTimes(1)
  })

  test('requestClusterStreamFocus triggers requestClusterKeyframe', async () => {
    await d.send(new SendCommand('requestClusterStreamFocus'))
    expect(aa.requestClusterKeyframe).toHaveBeenCalled()
  })

  test('selectDown / selectUp press/release DPAD_CENTER', async () => {
    await d.send(new SendCommand('selectDown'))
    await d.send(new SendCommand('selectUp'))
    expect(aa.sendButton).toHaveBeenCalledTimes(2)
    expect(aa.sendButton.mock.calls[0]).toEqual([23, true])
    expect(aa.sendButton.mock.calls[1]).toEqual([23, false])
  })

  test('voiceAssistant / voiceAssistantRelease press/release SEARCH', async () => {
    await d.send(new SendCommand('voiceAssistant'))
    await d.send(new SendCommand('voiceAssistantRelease'))
    expect(aa.sendButton.mock.calls[0]).toEqual([84, true])
    expect(aa.sendButton.mock.calls[1]).toEqual([84, false])
  })

  test('left / right send a rotary delta', async () => {
    await d.send(new SendCommand('left'))
    await d.send(new SendCommand('right'))
    expect(aa.sendRotary).toHaveBeenCalledWith(-1)
    expect(aa.sendRotary).toHaveBeenCalledWith(1)
  })

  test('knobLeft / knobRight send a rotary delta', async () => {
    await d.send(new SendCommand('knobLeft'))
    await d.send(new SendCommand('knobRight'))
    expect(aa.sendRotary).toHaveBeenCalledWith(-1)
    expect(aa.sendRotary).toHaveBeenCalledWith(1)
  })

  test.each([
    ['home', 3],
    ['back', 4],
    ['acceptPhone', 5],
    ['rejectPhone', 6],
    ['play', 126],
    ['pause', 127],
    ['playPause', 85],
    ['next', 87],
    ['prev', 88]
  ])('button mapping: %s → keycode %s', async (cmd, keycode) => {
    await d.send(new SendCommand(cmd as Parameters<typeof SendCommand>[0]))
    expect(aa.sendButton).toHaveBeenCalledWith(keycode, true)
    expect(aa.sendButton).toHaveBeenCalledWith(keycode, false)
  })

  test('up → DPAD_UP press+release', async () => {
    await d.send(new SendCommand('up'))
    expect(aa.sendButton).toHaveBeenCalledWith(19, true)
    expect(aa.sendButton).toHaveBeenCalledWith(19, false)
  })

  test('down → DPAD_DOWN press+release', async () => {
    await d.send(new SendCommand('down'))
    expect(aa.sendButton).toHaveBeenCalledWith(20, true)
    expect(aa.sendButton).toHaveBeenCalledWith(20, false)
  })

  test('releaseVideoFocus returns true without further action', async () => {
    const ok = await d.send(new SendCommand('releaseVideoFocus'))
    expect(ok).toBe(true)
    expect(aa.sendButton).not.toHaveBeenCalled()
  })

  test('unknown command is silently swallowed', async () => {
    const unmapped = Object.values(CommandMapping).find(
      (v): v is number =>
        typeof v === 'number' && v > 1000 && v !== CommandMapping.requestClusterStreamFocus
    )
    if (unmapped !== undefined) {
      const cmd = new SendCommand('home')
      vi.spyOn(cmd, 'getPayload').mockReturnValue(
        (() => {
          const b = Buffer.alloc(4)
          b.writeUInt32LE(unmapped, 0)
          return b
        })()
      )
      const ok = await d.send(cmd)
      expect(ok).toBe(true)
    }
  })
})

describe('AaSession.send — SendTouch + SendMultiTouch', () => {
  let d: AaSession
  let aa: MockAAStack

  beforeEach(() => {
    d = makeSession()
    aa = lastAaStack.instance!
  })

  test('SendTouch forwards a single pointer when in bounds', async () => {
    await d.send(new SendTouch(0.5, 0.5, TouchAction.Down))
    expect(aa.sendTouch).toHaveBeenCalled()
    const [action, pointers] = aa.sendTouch.mock.calls[0]
    expect(action).toBe(0) // TOUCH_ACTION.DOWN
    expect(pointers).toHaveLength(1)
  })

  test('SendMultiTouch forwards every in-window pointer', async () => {
    const msg = new SendMultiTouch([
      { id: 0, x: 0.1, y: 0.1, action: MultiTouchAction.Down },
      { id: 1, x: 0.5, y: 0.5, action: MultiTouchAction.Move }
    ])
    await d.send(msg)
    const [, pointers] = aa.sendTouch.mock.calls[0]
    expect(pointers).toHaveLength(2)
  })

  test('SendMultiTouch with empty list returns true without forwarding', async () => {
    const ok = await d.send(new SendMultiTouch([]))
    expect(ok).toBe(true)
    expect(aa.sendTouch).not.toHaveBeenCalled()
  })

  test('SendMultiTouch with Up action and >1 finger sends POINTER_UP', async () => {
    const msg = new SendMultiTouch([
      { id: 0, x: 0.1, y: 0.1, action: MultiTouchAction.Up },
      { id: 1, x: 0.5, y: 0.5, action: MultiTouchAction.Move }
    ])
    await d.send(msg)
    expect(aa.sendTouch.mock.calls[0][0]).toBe(6) // POINTER_UP
  })

  test('SendMultiTouch with single Down sends ACTION_DOWN', async () => {
    const msg = new SendMultiTouch([{ id: 0, x: 0.1, y: 0.1, action: MultiTouchAction.Down }])
    await d.send(msg)
    expect(aa.sendTouch.mock.calls[0][0]).toBe(0) // ACTION_DOWN
  })
})

describe('AaSession.send — shutdown messages', () => {
  test('SendDisconnectPhone calls AAStack.requestShutdown', async () => {
    const d = makeSession()
    await d.send(new SendDisconnectPhone())
    expect(lastAaStack.instance!.requestShutdown).toHaveBeenCalled()
  })

  test('SendCloseDongle calls AAStack.requestShutdown', async () => {
    const d = makeSession()
    await d.send(new SendCloseDongle())
    expect(lastAaStack.instance!.requestShutdown).toHaveBeenCalled()
  })
})

describe('AaSession — vehicle-data passthrough', () => {
  let d: AaSession
  let aa: MockAAStack
  beforeEach(() => {
    d = makeSession()
    aa = lastAaStack.instance!
  })

  test('all push methods forward to AAStack', () => {
    d.sendFuelData(50)
    d.sendSpeedData(13_000)
    d.sendRpmData(2_500_000)
    d.sendGearData(4)
    d.sendNightModeData(true)
    d.sendParkingBrakeData(false)
    d.sendLightData(1, false, 2)
    d.sendEnvironmentData(20_000, 1013_000, 0)
    d.sendOdometerData(120_000)
    d.sendDrivingStatusData(0)
    d.sendGpsLocationData({ latDeg: 52, lngDeg: 13 })
    d.sendVehicleEnergyModel(50_000, 30_000, 200_000, { maxChargePowerW: 11_000 })

    expect(aa.sendFuelData).toHaveBeenCalled()
    expect(aa.sendSpeedData).toHaveBeenCalled()
    expect(aa.sendRpmData).toHaveBeenCalled()
    expect(aa.sendGearData).toHaveBeenCalled()
    expect(aa.sendNightModeData).toHaveBeenCalled()
    expect(aa.sendParkingBrakeData).toHaveBeenCalled()
    expect(aa.sendLightData).toHaveBeenCalled()
    expect(aa.sendEnvironmentData).toHaveBeenCalled()
    expect(aa.sendOdometerData).toHaveBeenCalled()
    expect(aa.sendDrivingStatusData).toHaveBeenCalled()
    expect(aa.sendGpsLocationData).toHaveBeenCalled()
    expect(aa.sendVehicleEnergyModel).toHaveBeenCalled()
  })

  test('push methods are no-ops after close', async () => {
    const d2 = makeSession()
    await d2.close()
    expect(() => {
      d2.sendFuelData(0)
      d2.sendSpeedData(0)
      d2.sendGpsLocationData({ latDeg: 0, lngDeg: 0 })
    }).not.toThrow()
  })
})

describe('AaSession — microphone lifecycle', () => {
  test('voice-session START twice only starts mic once', () => {
    const d = makeSession()
    const internal = d as unknown as {
      _startMicCapture: (reason: string) => void
      _mic: MockMicrophone | null
    }
    internal._startMicCapture('a')
    const micA = internal._mic
    internal._startMicCapture('b')
    expect(internal._mic).toBe(micA)
    expect(micA!.start).toHaveBeenCalledTimes(1)
  })

  test('mic-stop when never started is a no-op', () => {
    const d = makeSession()
    expect(() =>
      (d as unknown as { _stopMicCapture: (r: string) => void })._stopMicCapture('x')
    ).not.toThrow()
  })

  test('mic data is forwarded to AAStack.sendMicPcm while active', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const internal = d as unknown as {
      _startMicCapture: (r: string) => void
      _mic: MockMicrophone | null
    }
    internal._startMicCapture('mic-start')
    internal._mic!.emit('data', Buffer.from([1, 2]))
    expect(aa.sendMicPcm).toHaveBeenCalledWith(Buffer.from([1, 2]))
  })

  test('mic data is dropped after _micActive flips off', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const internal = d as unknown as {
      _startMicCapture: (r: string) => void
      _stopMicCapture: (r: string) => void
      _mic: MockMicrophone | null
    }
    internal._startMicCapture('mic-start')
    internal._stopMicCapture('mic-stop')
    aa.sendMicPcm.mockClear()
    internal._mic!.emit('data', Buffer.from([3, 4]))
    expect(aa.sendMicPcm).not.toHaveBeenCalled()
  })
})

describe('AaSession — close error swallowing', () => {
  test('mic.stop throwing is swallowed', async () => {
    const d = makeSession()
    const internal = d as unknown as {
      _startMicCapture: (r: string) => void
      _mic: MockMicrophone | null
    }
    internal._startMicCapture('x')
    internal._mic!.stop.mockImplementation(function () {
      throw new Error('alsa eof')
    })
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('AAStack.requestShutdown rejecting is swallowed', async () => {
    const d = makeSession()
    lastAaStack.instance!.requestShutdown.mockRejectedValueOnce(new Error('no peer'))
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('wired bridge drain rejecting is swallowed', async () => {
    const bridge = new MockUsbAoapBridge()
    const d = makeSession({ wired: true, wiredBridge: bridge })
    bridge.drain.mockRejectedValueOnce(new Error('hung'))
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('wired bridge.stop() throwing is swallowed', async () => {
    const bridge = new MockUsbAoapBridge()
    const d = makeSession({ wired: true, wiredBridge: bridge })
    bridge.stop.mockImplementationOnce(async () => {
      throw new Error('USB hung')
    })
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('AAStack.stop throwing is swallowed', async () => {
    const d = makeSession()
    lastAaStack.instance!.stop.mockImplementationOnce(() => {
      throw new Error('half-open')
    })
    await expect(d.close()).resolves.toBeUndefined()
  })
})

describe('AaSession — bridge dep callbacks', () => {
  test('emitMessage closure forwards a "message" event from the AA stack', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const cb = vi.fn()
    d.on('message', cb)
    aa.emit('video-frame', Buffer.alloc(64), 0n) // Bridge → deps.emitMessage(VideoData)
    expect(cb).toHaveBeenCalled()
  })

  test('emitCodec closure forwards video-codec from the AA stack', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const cb = vi.fn()
    d.on('video-codec', cb)
    aa.emit('video-codec', 'h265')
    expect(cb).toHaveBeenCalledWith('h265')
  })

  test('startMic / stopMic deps wire to the internal mic capture', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    aa.emit('mic-start')
    const internal = d as unknown as { _micActive: boolean }
    expect(internal._micActive).toBe(true)
    aa.emit('mic-stop')
    expect(internal._micActive).toBe(false)
  })

  test('isClosed dep flips to true after close()', async () => {
    const d = makeSession()
    await d.close()
    expect((d as unknown as { _closed: boolean })._closed).toBe(true)
  })

  test('device-info from the stack surfaces a device-presence event', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const cb = vi.fn()
    d.on('device-presence', cb)
    aa.emit('device-info', { name: 'Pixel', model: 'P8', instanceId: 'i1', ip: '10.0.0.2' })
    expect(cb).toHaveBeenCalledWith(expect.objectContaining({ kind: 'device', name: 'Pixel' }))
  })
})

describe('AaSession — touch out-of-window handling', () => {
  test('SendTouch with out-of-window coordinates is swallowed', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const internal = d as unknown as {
      _touchInsetLeft: number
      _touchInsetRight: number
      _touchInsetTop: number
      _touchInsetBottom: number
      _touchW: number
      _touchH: number
    }
    internal._touchInsetLeft = 100
    internal._touchInsetRight = 100
    internal._touchInsetTop = 100
    internal._touchInsetBottom = 100
    internal._touchW = 200
    internal._touchH = 200
    const ok = await d.send(new SendTouch(0.01, 0.01, TouchAction.Down))
    expect(ok).toBe(true)
    expect(aa.sendTouch).not.toHaveBeenCalled()
  })

  test('SendMultiTouch where all pointers are out-of-window returns true', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const internal = d as unknown as {
      _touchInsetLeft: number
      _touchInsetRight: number
      _touchInsetTop: number
      _touchInsetBottom: number
      _touchW: number
      _touchH: number
    }
    internal._touchInsetLeft = 1000
    internal._touchInsetRight = 1000
    internal._touchInsetTop = 1000
    internal._touchInsetBottom = 1000
    internal._touchW = 100
    internal._touchH = 100
    const ok = await d.send(
      new SendMultiTouch([{ id: 0, x: 0, y: 0, action: MultiTouchAction.Down }])
    )
    expect(ok).toBe(true)
    expect(aa.sendTouch).not.toHaveBeenCalled()
  })
})

describe('AaSession — codec/night-mode setters during an active session', () => {
  test('updates AAStackConfig in place when set after construction', () => {
    const d = makeSession()
    d.setHevcSupported(true)
    d.setVp9Supported(true)
    d.setAv1Supported(true)
    d.setInitialNightMode(true)

    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.hevcSupported).toBe(true)
    expect(cfg.vp9Supported).toBe(true)
    expect(cfg.av1Supported).toBe(true)
    expect(cfg.initialNightMode).toBe(true)
  })

  test('setClusterStreamActive forwards to the AAStack', () => {
    const d = makeSession()
    lastAaStack.instance!.setClusterStreamActive.mockClear()
    d.setClusterStreamActive(false)
    expect(lastAaStack.instance!.setClusterStreamActive).toHaveBeenCalledWith(false)
  })

  test('requestKeyframe asks the stack for main + cluster keyframes', () => {
    const d = makeSession()
    d.requestKeyframe()
    expect(lastAaStack.instance!.requestMainKeyframe).toHaveBeenCalled()
    expect(lastAaStack.instance!.forceClusterKeyframe).toHaveBeenCalled()
  })
})
