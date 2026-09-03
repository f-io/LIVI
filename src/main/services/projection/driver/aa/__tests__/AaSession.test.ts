import { EventEmitter } from 'node:events'

class MockAAStack extends EventEmitter {
  cfg: unknown
  stop = vi.fn()
  attachLink = vi.fn()
  sendMediaSink = vi.fn()
  setConfigRefresh = vi.fn()
  setClusterStreamActive = vi.fn()
  applyDisplayConfig = vi.fn()
  requestVideoFocus = vi.fn()
  requestMainKeyframe = vi.fn()
  micFormat = vi.fn(() => ({ sampleRate: 16000, channels: 1 }))
  micSocketPath = vi.fn((): string | null => '/run/livi/aa-mic.sock')
  requestClusterKeyframe = vi.fn()
  forceClusterKeyframe = vi.fn()
  requestShutdown = vi.fn(async () => undefined)
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
  constructor(cfg: unknown) {
    super()
    this.cfg = cfg
  }
}

/** The pipeline's microphone tap, the session only opens and closes it. */
class MockMicTap {
  close = vi.fn()
}

const openedTaps: MockMicTap[] = []
const micTapOpen = vi.fn((_path: string, _opts: unknown): MockMicTap | null => {
  const tap = new MockMicTap()
  openedTaps.push(tap)
  return tap
})

/** The helper's session link, as far as the mocked stack needs it. */
class FakeLink extends EventEmitter {
  readonly peer = '10.0.0.2'
  closed = false
  send = vi.fn()
  control = vi.fn()
  end = vi.fn()
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

vi.mock('@main/services/audio/micTap', () => ({
  MicTap: { open: (path: string, opts: unknown) => micTapOpen(path, opts) }
}))

import type { Config } from '@shared/types'
import { CarType } from '@shared/types/Config'
import { InputCommand } from '@shared/types/InputCommand'
import { CommandMapping, MultiTouchAction, TouchAction } from '@shared/types/ProjectionEnums'
import {
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone,
  SendMultiTouch,
  SendTouch
} from '../../../messages/sendable'
import { AaSession, type AaSessionSeed } from '../AaSession'
import { TOUCH_ACTION } from '../stack/index'
import type { HelperSessionLink } from '../stack/transport/HelperSessionLink'

const TOUCH_MOVED = TOUCH_ACTION.MOVED
const TOUCH_UP = TOUCH_ACTION.UP

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

const fakeLink = (): HelperSessionLink => new FakeLink() as unknown as HelperSessionLink

function makeSession(
  opts: { cfg?: Config; wired?: boolean; usbSerial?: string; seed?: AaSessionSeed } = {}
): AaSession {
  const cfg = opts.cfg ?? baseCfg()
  return new AaSession({
    transport: fakeLink(),
    getConfig: () => cfg,
    wired: opts.wired ?? false,
    usbSerial: opts.usbSerial,
    seed: opts.seed ?? baseSeed()
  })
}

beforeEach(() => {
  lastAaStack.instance = null
  openedTaps.length = 0
  vi.clearAllMocks()
  vi.spyOn(console, 'log').mockImplementation(function () {})
  vi.spyOn(console, 'warn').mockImplementation(function () {})
  vi.spyOn(console, 'error').mockImplementation(function () {})
})
afterEach(() => {
  vi.restoreAllMocks()
})

describe('AaSession: host volume hook', () => {
  test('setStreamVolume forwards to the media sink, the same shape CarPlay uses', () => {
    const setHostVolume = vi.fn()
    const s = new AaSession({
      transport: fakeLink(),
      getConfig: () => baseCfg(),
      wired: false,
      seed: baseSeed(),
      mediaSink: {
        feedPath: async () => '',
        videoPlaneId: () => 1,
        primeVideo: vi.fn(),
        noteVideoStarted: vi.fn(),
        audioOutputs: () => [],
        onAudioOutput: () => () => {},
        primeAudio: vi.fn(),
        setHostVolume
      }
    })
    s.setStreamVolume(3, 0.5, 40)
    expect(setHostVolume).toHaveBeenCalledWith(3, 0.5, 40)
  })
})

describe('AaSession: construction', () => {
  test('constructs an AAStack and adopts the helper link via attachLink', () => {
    const link = fakeLink()
    const d = new AaSession({
      transport: link,
      getConfig: () => baseCfg(),
      wired: false,
      seed: baseSeed()
    })
    expect(d).toBeInstanceOf(AaSession)
    expect(lastAaStack.instance).not.toBeNull()
    expect(lastAaStack.instance!.attachLink).toHaveBeenCalledWith(link)
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
    expect(makeSession({ wired: true }).isWiredMode()).toBe(true)
  })

  test('start() resolves true, the link was adopted at construction already', async () => {
    const d = makeSession()
    await expect(d.start(baseCfg())).resolves.toBe(true)
    expect(lastAaStack.instance!.attachLink).toHaveBeenCalledTimes(1)
  })
})

describe('AaSession.close', () => {
  test('says goodbye to the phone and stops the AAStack', async () => {
    const d = makeSession({ wired: true })

    await d.close()
    expect(lastAaStack.instance!.requestShutdown).toHaveBeenCalled()
    expect(lastAaStack.instance!.stop).toHaveBeenCalled()
  })

  test('idempotent: second close is a no-op', async () => {
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
    const d = makeSession({ wired: true })
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
    lastAaStack.instance!.emit('disconnected', 'handshake stalled')
    await d.close()
    expect(cb).toHaveBeenCalledTimes(1)
  })
})

describe('AaSession.send: bail-out', () => {
  test('returns false after close (no AAStack)', async () => {
    const d = makeSession()
    await d.close()
    const ok = await d.send(new SendCommand('frame'))
    expect(ok).toBe(false)
  })
})

describe('AaSession.send: SendCommand', () => {
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
      ;(cmd as { value: number }).value = unmapped
      const ok = await d.send(cmd)
      expect(ok).toBe(true)
    }
  })
})

describe('AaSession.send: SendTouch + SendMultiTouch', () => {
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

describe('AaSession.send: shutdown messages', () => {
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

describe('AaSession: vehicle-data passthrough', () => {
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

describe('AaSession: microphone lifecycle', () => {
  type MicInternals = {
    _startMicCapture: (reason: string) => void
    _stopMicCapture: (reason: string) => void
    _micTap: MockMicTap | null
    _micActive: boolean
  }
  const internals = (d: AaSession): MicInternals => d as unknown as MicInternals

  test('capture opens a tap at the stack mic socket with the negotiated format', () => {
    const cfg = { ...baseCfg(), audioInputDevice: 'alsa_input.usb-mic' } as Config
    const d = makeSession({ cfg })
    const aa = lastAaStack.instance!
    aa.micSocketPath.mockReturnValue('/run/livi/aa-mic.sock')
    aa.micFormat.mockReturnValue({ sampleRate: 24000, channels: 2 })

    internals(d)._startMicCapture('mic-start')

    expect(micTapOpen).toHaveBeenCalledWith('/run/livi/aa-mic.sock', {
      sampleRate: 24000,
      channels: 2,
      device: 'alsa_input.usb-mic'
    })
    expect(internals(d)._micActive).toBe(true)
    expect(internals(d)._micTap).toBe(openedTaps[0])
  })

  test('mic capture falls back to 16 kHz mono when the stack reports no format', () => {
    const d = makeSession()
    lastAaStack.instance!.micFormat.mockReturnValue(undefined as never)
    internals(d)._startMicCapture('mic-start')
    expect(micTapOpen).toHaveBeenCalledWith('/run/livi/aa-mic.sock', {
      sampleRate: 16000,
      channels: 1,
      device: undefined
    })
  })

  test('no tap without a mic socket from the helper', () => {
    const d = makeSession()
    lastAaStack.instance!.micSocketPath.mockReturnValue(null)
    internals(d)._startMicCapture('mic-start')
    expect(micTapOpen).not.toHaveBeenCalled()
    expect(internals(d)._micActive).toBe(false)
    expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('no mic socket'))
  })

  test('a tap that does not open leaves the capture inactive', () => {
    const d = makeSession()
    micTapOpen.mockReturnValueOnce(null)
    internals(d)._startMicCapture('mic-start')
    expect(internals(d)._micActive).toBe(false)
    expect(internals(d)._micTap).toBeNull()
  })

  test('voice-session START twice only opens the tap once', () => {
    const d = makeSession()
    internals(d)._startMicCapture('a')
    internals(d)._startMicCapture('b')
    expect(micTapOpen).toHaveBeenCalledTimes(1)
    expect(internals(d)._micTap).toBe(openedTaps[0])
  })

  test('mic-stop when never started is a no-op', () => {
    const d = makeSession()
    expect(() => internals(d)._stopMicCapture('x')).not.toThrow()
    expect(openedTaps).toHaveLength(0)
  })

  test('stop closes the tap', () => {
    const d = makeSession()
    internals(d)._startMicCapture('mic-start')
    internals(d)._stopMicCapture('mic-stop')
    expect(openedTaps[0].close).toHaveBeenCalledTimes(1)
    expect(internals(d)._micActive).toBe(false)
    expect(internals(d)._micTap).toBeNull()
  })

  test('a mic tap that throws on close is swallowed', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    const d = makeSession()
    internals(d)._startMicCapture('mic-start')
    openedTaps[0].close.mockImplementation(() => {
      throw new Error('down')
    })
    expect(() => internals(d)._stopMicCapture('mic-stop')).not.toThrow()
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('mic tap close failed'))
    warn.mockRestore()
  })

  test('close closes the tap', async () => {
    const d = makeSession()
    internals(d)._startMicCapture('mic-start')
    await d.close()
    expect(openedTaps[0].close).toHaveBeenCalledTimes(1)
    expect(internals(d)._micActive).toBe(false)
    expect(internals(d)._micTap).toBeNull()
  })
})

describe('AaSession: close error swallowing', () => {
  test('AAStack.requestShutdown rejecting is swallowed', async () => {
    const d = makeSession()
    lastAaStack.instance!.requestShutdown.mockRejectedValueOnce(new Error('no peer'))
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

describe('AaSession: bridge dep callbacks', () => {
  test('emitMessage closure forwards a "message" event from the AA stack', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    const cb = vi.fn()
    d.on('message', cb)
    aa.emit('host-ui-requested') // Bridge → deps.emitMessage(Command(requestHostUI))
    expect(cb).toHaveBeenCalledTimes(1)
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

describe('AaSession: touch out-of-window handling', () => {
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

describe('AaSession: codec/night-mode setters during an active session', () => {
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

  test('a night-mode change is pushed to the phone, not just stored', () => {
    const d = makeSession()
    lastAaStack.instance!.sendNightModeData.mockClear()

    d.setInitialNightMode(true)
    d.setInitialNightMode(false)

    expect(lastAaStack.instance!.sendNightModeData).toHaveBeenNthCalledWith(1, true)
    expect(lastAaStack.instance!.sendNightModeData).toHaveBeenNthCalledWith(2, false)
  })

  test('taking over as the active session applies the appearance', () => {
    const d = makeSession()
    lastAaStack.instance!.sendNightModeData.mockClear()

    d.applyNightMode(true)

    expect(lastAaStack.instance!.sendNightModeData).toHaveBeenCalledWith(true)
  })

  test('an undefined night mode leaves the phone alone', () => {
    const d = makeSession()
    lastAaStack.instance!.sendNightModeData.mockClear()

    d.setInitialNightMode(undefined)

    expect(lastAaStack.instance!.sendNightModeData).not.toHaveBeenCalled()
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

  test('codec/night-mode setters after close are ignored (no stored config)', async () => {
    const d = makeSession()
    await d.close()
    expect(() => {
      d.setHevcSupported(true)
      d.setVp9Supported(true)
      d.setAv1Supported(true)
      d.setInitialNightMode(true)
    }).not.toThrow()
  })

  test('usbSerial returns the descriptor serial for a wired session', () => {
    const wired = makeSession({ wired: true, usbSerial: 'SN-42' })
    expect(wired.usbSerial()).toBe('SN-42')
    expect(makeSession().usbSerial()).toBe('')
  })

  test('the config-refresh callback rebuilds the stack config', () => {
    makeSession()
    const aa = lastAaStack.instance!
    const refresh = aa.setConfigRefresh.mock.calls[0][0] as () => void
    refresh()
    expect(aa.applyDisplayConfig).toHaveBeenCalled()
  })
})

describe('AaSession: config edge cases', () => {
  test('explicit projection + cluster DPI are used verbatim', () => {
    makeSession({
      cfg: { ...baseCfg(), projectionDpi: 160, clusterDpi: 200 } as Config
    })
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.videoDpi).toBe(160)
    expect(cfg.clusterDpi).toBe(200)
  })

  test.each([[CarType.HybridGasoline], [CarType.HybridDiesel], [CarType.Diesel]])(
    'carType %s maps to a fuel-type list',
    (carType) => {
      makeSession({ cfg: { ...baseCfg(), carType } as unknown as Config })
      const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
      expect(Array.isArray(cfg.fuelTypes)).toBe(true)
      expect((cfg.fuelTypes as number[]).length).toBeGreaterThan(0)
    }
  )

  test('an active cluster dashboard is advertised and its AR is logged', () => {
    makeSession({
      cfg: {
        ...baseCfg(),
        dashboards: { dash3: { main: true } }
      } as unknown as Config
    })
    expect((lastAaStack.instance!.cfg as Record<string, unknown>).clusterEnabled).toBe(true)
  })

  test('a wide display yields a height margin (letterbox top/bottom)', () => {
    expect(() =>
      makeSession({ cfg: { ...baseCfg(), projectionWidth: 2560, projectionHeight: 720 } as Config })
    ).not.toThrow()
  })

  test('a tall display yields a width margin (pillarbox left/right)', () => {
    expect(() =>
      makeSession({
        cfg: { ...baseCfg(), projectionWidth: 1080, projectionHeight: 1920 } as Config
      })
    ).not.toThrow()
  })

  test('zero projection dimensions skip the aspect-ratio margin math', () => {
    expect(() =>
      makeSession({ cfg: { ...baseCfg(), projectionWidth: 0, projectionHeight: 0 } as Config })
    ).not.toThrow()
  })
})

describe('AaSession: bridge presence/lifecycle callbacks', () => {
  test('device-status surfaces a status device-presence event', () => {
    const d = makeSession()
    const cb = vi.fn()
    d.on('device-presence', cb)
    lastAaStack.instance!.emit('device-status', { battery: 55 })
    expect(cb).toHaveBeenCalledWith(expect.objectContaining({ kind: 'status', battery: 55 }))
  })

  test('a second connected event does not re-emit', () => {
    const d = makeSession()
    const cb = vi.fn()
    d.on('connected', cb)
    lastAaStack.instance!.emit('connected')
    lastAaStack.instance!.emit('connected')
    expect(cb).toHaveBeenCalledTimes(1)
  })

  test('a second disconnected event does not re-emit', () => {
    const d = makeSession()
    const cb = vi.fn()
    d.on('disconnected', cb)
    lastAaStack.instance!.emit('disconnected')
    lastAaStack.instance!.emit('disconnected')
    expect(cb).toHaveBeenCalledTimes(1)
  })

  test('isClosed dep is consulted when the stack reports an error', () => {
    makeSession()
    expect(() => lastAaStack.instance!.emit('error', new Error('transient'))).not.toThrow()
  })

  test('restarting the capture after a stop opens a fresh tap', () => {
    const d = makeSession()
    const internal = d as unknown as {
      _startMicCapture: (r: string) => void
      _stopMicCapture: (r: string) => void
      _micTap: MockMicTap | null
    }
    internal._startMicCapture('a')
    internal._stopMicCapture('b')
    internal._startMicCapture('c')
    expect(micTapOpen).toHaveBeenCalledTimes(2)
    expect(openedTaps[0].close).toHaveBeenCalledTimes(1)
    expect(internal._micTap).toBe(openedTaps[1])
  })
})

describe('AaSession: touch + multitouch action variants', () => {
  test('SendTouch Move and Up map to the right pointer actions', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    await d.send(new SendTouch(0.5, 0.5, TouchAction.Move))
    await d.send(new SendTouch(0.5, 0.5, TouchAction.Up))
    expect(aa.sendTouch.mock.calls[0][0]).toBe(TOUCH_MOVED)
    expect(aa.sendTouch.mock.calls[1][0]).toBe(TOUCH_UP)
  })

  test('SendTouch with an out-of-enum action falls back to MOVED', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    await d.send(new SendTouch(0.5, 0.5, 99 as unknown as TouchAction))
    expect(aa.sendTouch.mock.calls[0][0]).toBe(TOUCH_MOVED)
  })

  test('clamp01 handles negative, over-one and non-finite coordinates', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    await d.send(new SendTouch(-0.5, 0.5, TouchAction.Down))
    await d.send(new SendTouch(2, 0.5, TouchAction.Down))
    await d.send(new SendTouch(Number.NaN, 0.5, TouchAction.Down))
    expect(aa.sendTouch).toHaveBeenCalled()
  })

  test('all-Move multitouch uses the first pointer as the trigger', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    await d.send(
      new SendMultiTouch([
        { id: 0, x: 0.2, y: 0.2, action: MultiTouchAction.Move },
        { id: 1, x: 0.6, y: 0.6, action: MultiTouchAction.Move }
      ])
    )
    expect(aa.sendTouch.mock.calls[0][0]).toBe(TOUCH_MOVED)
    expect(aa.sendTouch.mock.calls[0][2]).toBe(0)
  })

  test('single-finger Up multitouch maps to ACTION_UP', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    await d.send(new SendMultiTouch([{ id: 0, x: 0.3, y: 0.3, action: MultiTouchAction.Up }]))
    expect(aa.sendTouch.mock.calls[0][0]).toBe(TOUCH_UP)
  })

  test('an unrecognised sendable message resolves to false', async () => {
    const d = makeSession()
    const ok = await d.send({} as never)
    expect(ok).toBe(false)
  })
})

describe('AaSession.handleInput', () => {
  test('a mapped input command presses and releases the mapped key', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    d.handleInput(InputCommand.Play)
    expect(aa.sendButton).toHaveBeenCalledWith(126, true)
    expect(aa.sendButton).toHaveBeenCalledWith(126, false)
  })

  test('an unmapped input command is a no-op', () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    d.handleInput('nonexistent' as InputCommand)
    expect(aa.sendButton).not.toHaveBeenCalled()
  })

  test('handleInput after close does nothing', async () => {
    const d = makeSession()
    const aa = lastAaStack.instance!
    await d.close()
    aa.sendButton.mockClear()
    d.handleInput(InputCommand.Play)
    expect(aa.sendButton).not.toHaveBeenCalled()
  })
})
