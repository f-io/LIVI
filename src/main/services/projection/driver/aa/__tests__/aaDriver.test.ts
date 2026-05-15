import { EventEmitter } from 'node:events'

class MockAAStack extends EventEmitter {
  cfg: unknown
  start = jest.fn()
  stop = jest.fn()
  attachSocket = jest.fn()
  requestKeyframe = jest.fn()
  requestClusterKeyframe = jest.fn()
  requestShutdown = jest.fn(async () => undefined)
  sendTouch = jest.fn()
  sendButton = jest.fn()
  sendRotary = jest.fn()
  sendMicPcm = jest.fn()
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
  constructor(cfg: unknown) {
    super()
    this.cfg = cfg
  }
}

class MockSupervisor extends EventEmitter {
  start = jest.fn()
  stop = jest.fn(async () => undefined)
}

class MockUsbAoapBridge extends EventEmitter {
  start = jest.fn(async () => undefined)
  stop = jest.fn(async () => undefined)
  drain = jest.fn(async () => undefined)
  forceReenum = jest.fn(async () => undefined)
  constructor() {
    super()
  }
}

class MockMicrophone extends EventEmitter {
  start = jest.fn()
  stop = jest.fn()
}

class MockSocket extends EventEmitter {
  destroy = jest.fn()
}

const lastAaStack: { instance: MockAAStack | null } = { instance: null }
const lastBridge: { instance: MockUsbAoapBridge | null } = { instance: null }

jest.mock('../stack/index', () => {
  const real = jest.requireActual('../stack/index')
  return {
    ...real,
    AAStack: jest.fn().mockImplementation((cfg: unknown) => {
      const aa = new MockAAStack(cfg)
      lastAaStack.instance = aa
      return aa
    })
  }
})

jest.mock('../aaBluetoothSupervisor', () => ({
  AaBluetoothSupervisor: jest.fn().mockImplementation(() => new MockSupervisor())
}))

jest.mock('../stack/transport/UsbAoapBridge', () => ({
  UsbAoapBridge: jest.fn().mockImplementation(() => {
    const b = new MockUsbAoapBridge()
    lastBridge.instance = b
    return b
  })
}))

jest.mock('@main/services/audio', () => ({
  Microphone: jest.fn().mockImplementation(() => new MockMicrophone())
}))

jest.mock('node:net', () => ({
  createConnection: jest.fn(() => new MockSocket())
}))

import * as net from 'node:net'
import type { DongleConfig } from '@shared/types'
import { CommandMapping, MultiTouchAction, TouchAction } from '@shared/types/ProjectionEnums'
import {
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone,
  SendMultiTouch,
  SendTouch
} from '../../../messages/sendable'
import { AaDriver } from '../aaDriver'

const baseCfg = (): DongleConfig =>
  ({
    width: 1280,
    height: 720,
    fps: 30,
    dpi: 0,
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
  }) as unknown as DongleConfig

const fakeUsbDevice = () =>
  ({
    deviceDescriptor: { idVendor: 0x18d1, idProduct: 0x4ee1 }
  }) as unknown as import('usb').Device

beforeEach(() => {
  lastAaStack.instance = null
  lastBridge.instance = null
  jest.clearAllMocks()
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => {
  jest.restoreAllMocks()
})

describe('AaDriver.start — wireless', () => {
  test('returns true and constructs an AAStack', async () => {
    const d = new AaDriver()
    const ok = await d.start(baseCfg())
    expect(ok).toBe(true)
    expect(lastAaStack.instance).not.toBeNull()
    expect(lastAaStack.instance!.start).toHaveBeenCalled()
  })

  test('starts the BT supervisor on the wireless path', async () => {
    const supervisor = new MockSupervisor()
    const d = new AaDriver({ supervisor: supervisor as unknown as never })
    await d.start(baseCfg())
    expect(supervisor.start).toHaveBeenCalled()
  })

  test('idempotent: a second start returns true without re-constructing AAStack', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const first = lastAaStack.instance
    await d.start(baseCfg())
    expect(lastAaStack.instance).toBe(first)
  })

  test('config seed: hevc/vp9/av1/nightMode are propagated to AAStackConfig', async () => {
    const d = new AaDriver()
    d.setHevcSupported(true)
    d.setVp9Supported(true)
    d.setAv1Supported(true)
    d.setInitialNightMode(true)

    await d.start(baseCfg())
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.hevcSupported).toBe(true)
    expect(cfg.vp9Supported).toBe(true)
    expect(cfg.av1Supported).toBe(true)
    expect(cfg.initialNightMode).toBe(true)
  })

  test('config seed: fps=60 → videoFps=60', async () => {
    const d = new AaDriver()
    await d.start({ ...baseCfg(), fps: 60 } as DongleConfig)
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.videoFps).toBe(60)
  })

  test('config seed: hand=1 → driverPosition=1', async () => {
    const d = new AaDriver()
    await d.start({ ...baseCfg(), hand: 1 } as DongleConfig)
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.driverPosition).toBe(1)
  })

  test('config seed: empty carName falls back to "LIVI"', async () => {
    const d = new AaDriver()
    await d.start({ ...baseCfg(), carName: '   ' } as unknown as DongleConfig)
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.huName).toBe('LIVI')
    expect(cfg.wifiSsid).toBe('LIVI')
  })

  test('config seed: wifiPassword defaults to "12345678" when empty', async () => {
    const d = new AaDriver()
    await d.start({ ...baseCfg(), wifiPassword: '' } as DongleConfig)
    const cfg = lastAaStack.instance!.cfg as Record<string, unknown>
    expect(cfg.wifiPassword).toBe('12345678')
  })
})

describe('AaDriver.start — wired', () => {
  test('starts the UsbAoapBridge instead of the supervisor', async () => {
    const supervisor = new MockSupervisor()
    const d = new AaDriver({ supervisor: supervisor as unknown as never })
    d.setWiredDevice(fakeUsbDevice())

    const ok = await d.start(baseCfg())
    expect(ok).toBe(true)
    expect(supervisor.start).not.toHaveBeenCalled()
    expect(lastBridge.instance).not.toBeNull()
    expect(lastBridge.instance!.start).toHaveBeenCalled()
  })

  test('isWiredMode reflects the setter', () => {
    const d = new AaDriver()
    expect(d.isWiredMode()).toBe(false)
    d.setWiredDevice(fakeUsbDevice())
    expect(d.isWiredMode()).toBe(true)
    d.setWiredDevice(null)
    expect(d.isWiredMode()).toBe(false)
  })

  test('UsbAoapBridge "ready" connects to the loopback and attaches to AAStack', async () => {
    const d = new AaDriver()
    d.setWiredDevice(fakeUsbDevice())
    await d.start(baseCfg())

    const bridge = lastBridge.instance!
    bridge.emit('ready', { host: '127.0.0.1', port: 5278 })
    expect(net.createConnection as jest.Mock).toHaveBeenCalled()

    const sock = (net.createConnection as jest.Mock).mock.results[0].value as MockSocket
    sock.emit('connect')
    expect(lastAaStack.instance!.attachSocket).toHaveBeenCalledWith(sock)
  })

  test('bridge.start rejection → start() returns false', async () => {
    // Make the next UsbAoapBridge.start reject
    const { UsbAoapBridge } = jest.requireMock('../stack/transport/UsbAoapBridge') as {
      UsbAoapBridge: jest.Mock
    }
    UsbAoapBridge.mockImplementationOnce(() => {
      const b = new MockUsbAoapBridge()
      b.start = jest.fn(async () => {
        throw new Error('init failed')
      })
      lastBridge.instance = b
      return b
    })

    const d = new AaDriver()
    d.setWiredDevice(fakeUsbDevice())
    const ok = await d.start(baseCfg())
    expect(ok).toBe(false)
  })
})

describe('AaDriver.restartStack', () => {
  test('stops then restarts the AAStack', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    aa.start.mockClear()
    await d.restartStack()
    expect(aa.stop).toHaveBeenCalled()
    expect(aa.start).toHaveBeenCalled()
  })

  test('no-op when no AAStack', async () => {
    const d = new AaDriver()
    await expect(d.restartStack()).resolves.toBeUndefined()
  })
})

describe('AaDriver.close', () => {
  test('stops mic + bridge + supervisor + AAStack and clears refs', async () => {
    const supervisor = new MockSupervisor()
    const d = new AaDriver({ supervisor: supervisor as unknown as never })
    d.setWiredDevice(fakeUsbDevice())
    await d.start(baseCfg())

    await d.close()
    expect(lastAaStack.instance!.requestShutdown).toHaveBeenCalled()
    expect(lastAaStack.instance!.stop).toHaveBeenCalled()
    expect(lastBridge.instance!.drain).toHaveBeenCalled()
    expect(supervisor.stop).toHaveBeenCalled()
  })

  test('idempotent — second close is a no-op', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    await d.close()
    await expect(d.close()).resolves.toBeUndefined()
  })
})

describe('AaDriver.send — bail-out', () => {
  test('returns false when no AAStack is active', async () => {
    const d = new AaDriver()
    const ok = await d.send(new SendCommand('frame'))
    expect(ok).toBe(false)
  })
})

describe('AaDriver.send — SendCommand', () => {
  let d: AaDriver
  let aa: MockAAStack
  beforeEach(async () => {
    d = new AaDriver()
    await d.start(baseCfg())
    aa = lastAaStack.instance!
  })

  test('frame triggers requestKeyframe', async () => {
    jest.useFakeTimers()
    await d.send(new SendCommand('frame'))
    expect(aa.requestKeyframe).toHaveBeenCalledTimes(1)
    jest.advanceTimersByTime(500)
    expect(aa.requestKeyframe).toHaveBeenCalledTimes(2)
    jest.useRealTimers()
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

  test('up → DPAD_LEFT press+release', async () => {
    await d.send(new SendCommand('up'))
    expect(aa.sendButton).toHaveBeenCalledWith(21, true)
    expect(aa.sendButton).toHaveBeenCalledWith(21, false)
  })

  test('down → DPAD_RIGHT press+release', async () => {
    await d.send(new SendCommand('down'))
    expect(aa.sendButton).toHaveBeenCalledWith(22, true)
    expect(aa.sendButton).toHaveBeenCalledWith(22, false)
  })

  test('releaseVideoFocus returns true without further action', async () => {
    const ok = await d.send(new SendCommand('releaseVideoFocus'))
    expect(ok).toBe(true)
    expect(aa.sendButton).not.toHaveBeenCalled()
  })

  test('unknown command is silently swallowed', async () => {
    // Use a CommandMapping value that's not mapped — pick a high one
    const unmapped = Object.values(CommandMapping).find(
      (v): v is number =>
        typeof v === 'number' && v > 1000 && v !== CommandMapping.requestClusterStreamFocus
    )
    if (unmapped !== undefined) {
      const cmd = new SendCommand('home') // placeholder; we'll inject the raw cmd
      jest.spyOn(cmd, 'getPayload').mockReturnValue(
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

describe('AaDriver.send — SendTouch + SendMultiTouch', () => {
  let d: AaDriver
  let aa: MockAAStack

  beforeEach(async () => {
    d = new AaDriver()
    await d.start(baseCfg())
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

describe('AaDriver.send — shutdown messages', () => {
  test('SendDisconnectPhone calls AAStack.requestShutdown', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    await d.send(new SendDisconnectPhone())
    expect(aa.requestShutdown).toHaveBeenCalled()
  })

  test('SendCloseDongle calls AAStack.requestShutdown', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    await d.send(new SendCloseDongle())
    expect(aa.requestShutdown).toHaveBeenCalled()
  })
})

describe('AaDriver — vehicle-data passthrough', () => {
  let d: AaDriver
  let aa: MockAAStack
  beforeEach(async () => {
    d = new AaDriver()
    await d.start(baseCfg())
    aa = lastAaStack.instance!
  })

  test('all push methods forward to AAStack when started', () => {
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

  test('push methods are no-ops when AAStack is not active', () => {
    const d2 = new AaDriver()
    expect(() => {
      d2.sendFuelData(0)
      d2.sendSpeedData(0)
      d2.sendGpsLocationData({ latDeg: 0, lngDeg: 0 })
    }).not.toThrow()
  })
})

describe('AaDriver — microphone lifecycle', () => {
  test('voice-session START twice only starts mic once', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const _internal = d as unknown as {
      _startMicCapture: (reason: string) => void
      _mic: MockMicrophone | null
      _micActive: boolean
    }
    _internal._startMicCapture('a')
    const micA = _internal._mic
    _internal._startMicCapture('b')
    expect(_internal._mic).toBe(micA)
    expect(micA!.start).toHaveBeenCalledTimes(1)
  })

  test('mic-stop when never started is a no-op', () => {
    const d = new AaDriver()
    expect(() =>
      (d as unknown as { _stopMicCapture: (r: string) => void })._stopMicCapture('x')
    ).not.toThrow()
  })

  test('mic data is forwarded to AAStack.sendMicPcm while active', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    const internal = d as unknown as {
      _startMicCapture: (r: string) => void
      _mic: MockMicrophone | null
    }
    internal._startMicCapture('mic-start')
    internal._mic!.emit('data', Buffer.from([1, 2]))
    expect(aa.sendMicPcm).toHaveBeenCalledWith(Buffer.from([1, 2]))
  })

  test('mic data is dropped after _micActive flips off', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
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

describe('AaDriver — close error swallowing', () => {
  test('mic.stop throwing is swallowed', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const internal = d as unknown as {
      _startMicCapture: (r: string) => void
      _mic: MockMicrophone | null
    }
    internal._startMicCapture('x')
    internal._mic!.stop.mockImplementation(() => {
      throw new Error('alsa eof')
    })
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('AAStack.requestShutdown rejecting is swallowed', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    aa.requestShutdown.mockRejectedValueOnce(new Error('no peer'))
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('wired bridge drain rejecting is swallowed', async () => {
    const d = new AaDriver()
    d.setWiredDevice(fakeUsbDevice())
    await d.start(baseCfg())
    lastBridge.instance!.drain.mockRejectedValueOnce(new Error('hung'))
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('AAStack.stop throwing is swallowed', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    aa.stop.mockImplementationOnce(() => {
      throw new Error('half-open')
    })
    await expect(d.close()).resolves.toBeUndefined()
  })
})

describe('AaDriver — bridge dep callbacks', () => {
  test('emitMessage closure forwards a "message" event from the AA stack', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    const cb = jest.fn()
    d.on('message', cb)
    aa.emit('connected') // triggers Bridge → deps.emitMessage(DongleReady)
    expect(cb).toHaveBeenCalled()
  })

  test('emitCodec closure forwards video-codec from the AA stack', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    const cb = jest.fn()
    d.on('video-codec', cb)
    aa.emit('video-codec', 'h265')
    expect(cb).toHaveBeenCalledWith('h265')
  })

  test('startMic / stopMic deps wire to the internal mic capture', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const aa = lastAaStack.instance!
    aa.emit('mic-start')
    const internal = d as unknown as { _micActive: boolean }
    expect(internal._micActive).toBe(true)
    aa.emit('mic-stop')
    expect(internal._micActive).toBe(false)
  })

  test('isClosed dep flips to true after close()', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
    const internal = d as unknown as { _bridge: { wire: jest.Mock } }
    // Bridge was constructed; trigger close()
    await d.close()
    expect((d as unknown as { _closed: boolean })._closed).toBe(true)
    void internal
  })
})

describe('AaDriver — touch out-of-window handling', () => {
  test('SendTouch with out-of-window coordinates is swallowed', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
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
    const d = new AaDriver()
    await d.start(baseCfg())
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

describe('AaDriver.close — wired-bridge + supervisor teardown errors', () => {
  test('wired bridge.stop() throwing is swallowed', async () => {
    const d = new AaDriver()
    d.setWiredDevice(fakeUsbDevice())
    await d.start(baseCfg())
    lastBridge.instance!.stop.mockImplementationOnce(async () => {
      throw new Error('USB hung')
    })
    await expect(d.close()).resolves.toBeUndefined()
  })

  test('supervisor.stop() throwing is swallowed', async () => {
    const supervisor = new MockSupervisor()
    supervisor.stop = jest.fn(async () => {
      throw new Error('python crashed')
    })
    const d = new AaDriver({ supervisor: supervisor as unknown as never })
    await d.start(baseCfg())
    await expect(d.close()).resolves.toBeUndefined()
  })
})

describe('AaDriver — codec/night-mode setters during an active session', () => {
  test('updates AAStackConfig in place when set after start', async () => {
    const d = new AaDriver()
    await d.start(baseCfg())
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
})
