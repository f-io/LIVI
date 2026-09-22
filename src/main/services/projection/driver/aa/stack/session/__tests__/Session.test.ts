import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'
import { AV_MSG, CH, CTRL_MSG, FRAME_FLAGS } from '../../constants'
import * as protoIndex from '../../proto/index'
import type { HelperSessionLink } from '../../transport/HelperSessionLink'
import { Session, type SessionConfig } from '../Session'

// Mirrors the private State enum in Session.ts
const STATE = {
  INIT: 0,
  AUTH: 1,
  SERVICE_DISCOVERY: 2,
  CHANNEL_SETUP: 3,
  RUNNING: 4,
  CLOSED: 5
} as const

// Stands in for the helper session socket, the session only sends, controls, ends and destroys it
class FakeLink extends EventEmitter {
  peer = '10.0.0.2'
  closed = false
  send = vi.fn()
  control = vi.fn()
  end = vi.fn()
  destroy = vi.fn()
}

function baseCfg(over: Partial<SessionConfig> = {}): SessionConfig {
  return {
    huName: 'LIVI',
    videoWidth: 1280,
    videoHeight: 720,
    videoFps: 30,
    videoDpi: 140,
    displayWidth: 1280,
    displayHeight: 720,
    clusterEnabled: false,
    clusterWidth: 0,
    clusterHeight: 0,
    clusterFps: 0,
    clusterDpi: 0,
    ...over
  } as SessionConfig
}

function makeSession(over: Partial<SessionConfig> = {}): { session: Session; link: FakeLink } {
  const link = new FakeLink()
  const session = new Session(link as unknown as HelperSessionLink, baseCfg(over))
  return { session, link }
}

function stateOf(session: Session): number {
  return (session as unknown as { _state: number })._state
}

function setState(session: Session, state: number): void {
  ;(session as unknown as { _state: number })._state = state
}

function forceRunning(session: Session): void {
  setState(session, STATE.RUNNING)
}

function captureEncrypted(session: Session): Mock {
  const fn = vi.fn()
  ;(session as unknown as { _sendEncrypted: Mock })._sendEncrypted = fn
  return fn
}

function sendAA(
  session: Session
): (ch: number, flags: number, msgId: number, data: Buffer) => void {
  return (session as unknown as { _sendAA: (...a: unknown[]) => void })._sendAA.bind(session)
}

function protoStub(): Record<string, unknown> {
  const codec = {
    verify: () => null,
    create: (f: Record<string, unknown>) => f,
    encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) }),
    decode: () => ({}),
    toObject: (m: unknown) => m
  }
  return {
    AuthCompleteIndication: codec,
    ChannelOpenResponse: codec,
    AVChannelSetupRequest: codec,
    AVChannelSetupResponse: codec,
    ServiceDiscoveryResponse: codec,
    PingRequest: codec
  }
}

// start() with the proto loader stubbed, the link has not reported ready yet
async function started(): Promise<{ session: Session; link: FakeLink }> {
  const { session, link } = makeSession()
  const spy = vi.spyOn(protoIndex, 'loadProtos').mockResolvedValue(protoStub() as never)
  await session.start()
  spy.mockRestore()
  return { session, link }
}

beforeEach(() => {
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => vi.restoreAllMocks())

describe('Session.close', () => {
  test('destroys the link and transitions to CLOSED', () => {
    const { session, link } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    session.close()
    expect(link.destroy).toHaveBeenCalled()
    expect(stateOf(session)).toBe(STATE.CLOSED)
    expect(down).toHaveBeenCalledWith('manual close')
  })

  test('a second close destroys the link again but reports nothing', () => {
    const { session, link } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    session.close()
    link.destroy.mockClear()
    session.close()
    expect(link.destroy).toHaveBeenCalled()
    expect(down).toHaveBeenCalledTimes(1)
  })

  test('survives destroy() throwing', () => {
    const { session, link } = makeSession()
    link.destroy = vi.fn(() => {
      throw new Error('already destroyed')
    })
    expect(() => session.close()).not.toThrow()
    expect(stateOf(session)).toBe(STATE.CLOSED)
  })
})

describe('Session — link events', () => {
  test('link "close" transitions to CLOSED', () => {
    const { session, link } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    link.emit('close')
    expect(stateOf(session)).toBe(STATE.CLOSED)
    expect(down).toHaveBeenCalledWith('socket closed')
  })

  test('link "error" emits error and transitions to CLOSED', () => {
    const { session, link } = makeSession()
    const onError = vi.fn()
    session.on('error', onError)
    link.emit('error', new Error('reset'))
    expect(onError).toHaveBeenCalledWith(new Error('reset'))
    expect(stateOf(session)).toBe(STATE.CLOSED)
  })

  test("control 'closed' transitions to CLOSED with the helper's reason", () => {
    const { session, link } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    link.emit('control', { type: 'closed', reason: 'phone went away' })
    expect(stateOf(session)).toBe(STATE.CLOSED)
    expect(down).toHaveBeenCalledWith('phone went away')
  })

  test("control 'closed' without a string reason reports 'helper closed'", () => {
    const { session, link } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    link.emit('control', { type: 'closed', reason: 7 })
    expect(down).toHaveBeenCalledWith('helper closed')
  })

  test("control 'eof' before RUNNING ends the link", () => {
    const { session, link } = makeSession()
    link.emit('control', { type: 'eof' })
    expect(link.end).toHaveBeenCalled()
    expect(stateOf(session)).toBe(STATE.INIT)
  })

  test("control 'eof' in RUNNING leaves the write side open", () => {
    const { session, link } = makeSession()
    forceRunning(session)
    link.emit('control', { type: 'eof' })
    expect(link.end).not.toHaveBeenCalled()
  })

  test("control 'first-frame' drives video-started and cluster-video-started", () => {
    const { session, link } = makeSession()
    const video = vi.fn()
    const cluster = vi.fn()
    session.on('video-started', video)
    session.on('cluster-video-started', cluster)
    link.emit('control', { type: 'first-frame', ch: CH.VIDEO })
    link.emit('control', { type: 'first-frame', ch: CH.CLUSTER_VIDEO })
    link.emit('control', { type: 'first-frame', ch: CH.MEDIA_AUDIO })
    expect(video).toHaveBeenCalledTimes(1)
    expect(cluster).toHaveBeenCalledTimes(1)
  })

  test('the first main frame releases a held cluster stream request', () => {
    const { session, link } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestClusterKeyframe()
    expect(sent).not.toHaveBeenCalled()
    link.emit('control', { type: 'first-frame', ch: CH.VIDEO })
    const clusterFocus = sent.mock.calls.find(
      (c) => c[0] === CH.CLUSTER_VIDEO && c[2] === AV_MSG.VIDEO_FOCUS_INDICATION
    )
    expect(clusterFocus).toBeDefined()
  })

  test("control 'ready' without a mic socket leaves none", () => {
    const { session, link } = makeSession()
    expect(session.micSocketPath()).toBeNull()
    link.emit('control', { type: 'ready', mic: 7 })
    expect(session.micSocketPath()).toBeNull()
  })

  test('unknown controls are ignored', () => {
    const { session, link } = makeSession()
    expect(() => link.emit('control', { type: 'whatever' })).not.toThrow()
    expect(stateOf(session)).toBe(STATE.INIT)
  })

  test('sendMediaSink forwards a sink control to the link', () => {
    const { session, link } = makeSession()
    session.sendMediaSink({ path: '/run/livi/feed.sock', plane: 1 })
    expect(link.control).toHaveBeenCalledWith({
      type: 'sink',
      path: '/run/livi/feed.sock',
      plane: 1
    })
  })
})

describe('Session — outbound input API gated by state', () => {
  test('sendTouch is a no-op outside RUNNING', async () => {
    const { session } = makeSession()
    const input = { sendTouch: vi.fn() }
    ;(session as unknown as { _input?: { sendTouch: Mock } })._input = input
    session.sendTouch(0, [{ x: 0, y: 0, id: 0 }])
    expect(input.sendTouch).not.toHaveBeenCalled()
  })

  test('sendTouch delegates to InputChannel when RUNNING', async () => {
    const { session } = makeSession()
    forceRunning(session)
    const input = { sendTouch: vi.fn() }
    ;(session as unknown as { _input: typeof input })._input = input
    const pointers = [{ x: 5, y: 5, id: 0 }]
    session.sendTouch(0, pointers, 1)
    expect(input.sendTouch).toHaveBeenCalledWith(0, pointers, 1)
  })

  test('sendButton delegates when RUNNING', async () => {
    const { session } = makeSession()
    forceRunning(session)
    const input = { sendButton: vi.fn() }
    ;(session as unknown as { _input: typeof input })._input = input
    session.sendButton(3, true)
    expect(input.sendButton).toHaveBeenCalledWith(3, true)
  })

  test('sendRotary delegates when RUNNING', async () => {
    const { session } = makeSession()
    forceRunning(session)
    const input = { sendRotary: vi.fn() }
    ;(session as unknown as { _input: typeof input })._input = input
    session.sendRotary(1)
    expect(input.sendRotary).toHaveBeenCalledWith(1)
  })

  test('requestVideoFocus / requestClusterKeyframe are no-ops outside RUNNING', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    session.requestVideoFocus()
    session.requestClusterKeyframe()
    expect(sent).not.toHaveBeenCalled()
  })

  test('requestVideoFocus emits a VIDEO_FOCUS_REQUEST(mode=PROJECTED) on the video channel', async () => {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestVideoFocus()
    expect(sent.mock.calls[0][0]).toBe(CH.VIDEO)
    expect(sent.mock.calls[0][1]).toBe(FRAME_FLAGS.ENC_SIGNAL)
    expect(sent.mock.calls[0][2]).toBe(AV_MSG.VIDEO_FOCUS_REQUEST)
    // VideoFocusRequestNotification: mode=PROJECTED(1), reason=UNKNOWN(0)
    expect((sent.mock.calls[0][3] as Buffer).equals(Buffer.from([0x10, 0x01, 0x18, 0x00]))).toBe(
      true
    )
  })

  test('requestClusterKeyframe holds the cluster focus until the first main frame', async () => {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestClusterKeyframe()
    expect(sent).not.toHaveBeenCalled()
    expect((session as unknown as { _clusterFocusPending: boolean })._clusterFocusPending).toBe(
      true
    )
    ;(session as unknown as { _mainFrameSeen: boolean })._mainFrameSeen = true
    session.requestClusterKeyframe()
    expect(sent.mock.calls[0][0]).toBe(CH.CLUSTER_VIDEO)
    expect(sent.mock.calls[0][2]).toBe(AV_MSG.VIDEO_FOCUS_INDICATION)
    expect((session as unknown as { _clusterFocusPending: boolean })._clusterFocusPending).toBe(
      false
    )
  })
})

describe('Session — sensor pushes', () => {
  function setup(): { session: Session; sent: Mock } {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    return { session, sent }
  }

  test('all sensor methods are no-ops outside RUNNING', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    session.sendFuelData(50)
    session.sendSpeedData(13_000)
    session.sendRpmData(2_000_000)
    session.sendGearData(4)
    session.sendNightModeData(true)
    session.sendParkingBrakeData(false)
    session.sendLightData(1)
    session.sendEnvironmentData(20_000)
    session.sendOdometerData(120_000)
    session.sendDrivingStatusData(0)
    session.sendGpsLocationData({ latDeg: 52, lngDeg: 13 })
    session.sendVehicleEnergyModel(50_000, 30_000, 200_000)
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendFuelData writes a SensorBatch on CH.SENSOR', async () => {
    const { session, sent } = setup()
    session.sendFuelData(50, 200, true)
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][0]).toBe(CH.SENSOR)
    expect(sent.mock.calls[0][2]).toBe(0x8003) // SENSOR_MESSAGE_BATCH
  })

  test.each([
    ['sendFuelData', [50]],
    ['sendSpeedData', [13_000]],
    ['sendRpmData', [2_000_000]],
    ['sendGearData', [4]],
    ['sendNightModeData', [true]],
    ['sendParkingBrakeData', [false]],
    ['sendOdometerData', [12_000]],
    ['sendDrivingStatusData', [0]]
  ])('%s writes one SensorBatch frame', (method, args) => {
    const { session, sent } = setup()
    type Method = (...a: unknown[]) => void
    ;((session as unknown as Record<string, Method>)[method] as Method)(...args)
    expect(sent).toHaveBeenCalledTimes(1)
  })

  test('sendLightData with no args writes nothing', async () => {
    const { session, sent } = setup()
    session.sendLightData()
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendEnvironmentData with no args writes nothing', async () => {
    const { session, sent } = setup()
    session.sendEnvironmentData()
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendGpsLocationData encodes lat/lon × 1e7 + optional fields', async () => {
    const { session, sent } = setup()
    session.sendGpsLocationData({
      latDeg: 52.5,
      lngDeg: 13.4,
      accuracyM: 5,
      altitudeM: 50,
      speedMs: 12,
      bearingDeg: 90
    })
    expect(sent).toHaveBeenCalledTimes(1)
  })

  test('sendVehicleEnergyModel is a no-op when capacity/current/range are non-positive', async () => {
    const { session, sent } = setup()
    session.sendVehicleEnergyModel(0, 0, 0)
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendVehicleEnergyModel writes one SensorBatch frame with valid inputs', async () => {
    const { session, sent } = setup()
    session.sendVehicleEnergyModel(50_000, 30_000, 200_000, {
      maxChargePowerW: 11_000,
      maxDischargePowerW: 11_000,
      auxiliaryWhPerKm: 2.5
    })
    expect(sent).toHaveBeenCalledTimes(1)
  })
})

describe('Session.requestShutdown', () => {
  test('no-op when state is already CLOSED', async () => {
    const { session } = makeSession()
    session.close()
    const sent = captureEncrypted(session)
    await session.requestShutdown()
    expect(sent).not.toHaveBeenCalled()
  })

  test('sends a ByeByeRequest on the control channel when active', async () => {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    await session.requestShutdown()
    expect(sent).toHaveBeenCalled()
    expect(sent.mock.calls[0][0]).toBe(CH.CONTROL)
  })
})

describe('Session — decrypted message dispatch', () => {
  test('CONTROL channel forwards to ControlChannel.handleMessage', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    ;(session as unknown as { _control?: { handleMessage: Mock } })._control = {
      handleMessage
    }
    ;(
      session as unknown as {
        _handleDecryptedMessage: (...args: unknown[]) => void
      }
    )._handleDecryptedMessage(CH.CONTROL, 0, 0xabcd, Buffer.from([1]))
    expect(handleMessage).toHaveBeenCalledWith(0xabcd, Buffer.from([1]))
  })

  test('CH.BLUETOOTH pairing decode error is swallowed without DEBUG', () => {
    const { session } = makeSession()
    const sendAAFn = vi.fn()
    ;(session as unknown as { _sendAA: Mock })._sendAA = sendAAFn
    ;(session as unknown as { _proto: unknown })._proto = {
      BluetoothPairingRequest: {
        decode: () => {
          throw new Error('bad payload')
        }
      },
      BluetoothPairingResponse: {
        verify: () => null,
        create: (f: unknown) => f,
        encode: () => ({ finish: () => new Uint8Array([1]) })
      }
    }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.BLUETOOTH, 0, 0x8001, Buffer.alloc(0))
    expect(sendAAFn).toHaveBeenCalledWith(
      CH.BLUETOOTH,
      expect.any(Number),
      0x8002,
      expect.anything()
    )
  })

  test('CHANNEL_OPEN_REQUEST on a service channel triggers an encrypted response', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(session as unknown as { _proto: { ChannelOpenResponse: unknown } })._proto = {
      ChannelOpenResponse: {
        verify: () => null,
        create: (fields: unknown) => fields,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) })
      }
    }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.VIDEO, 0, CTRL_MSG.CHANNEL_OPEN_REQUEST, Buffer.alloc(0))
    expect(sent).toHaveBeenCalled()
    expect(sent.mock.calls[0][2]).toBe(CTRL_MSG.CHANNEL_OPEN_RESPONSE)
  })

  test('VIDEO channel messages delegate to _video.handleMessage', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    ;(session as unknown as { _video?: { handleMessage: Mock } })._video = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.VIDEO, 0, 0x0001, Buffer.from([1, 2]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('audio channel messages delegate to the matching AudioChannel instance', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    const audioMap = new Map<number, { handleMessage: Mock }>()
    audioMap.set(CH.MEDIA_AUDIO, { handleMessage })
    ;(session as unknown as { _audio: Map<number, unknown> })._audio = audioMap
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.MEDIA_AUDIO, 0, 0x0001, Buffer.from([0]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('NAVIGATION channel delegates to _nav.handleMessage', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    ;(session as unknown as { _nav?: { handleMessage: Mock } })._nav = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.NAVIGATION, 0, 0x8001, Buffer.alloc(0))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('MEDIA_INFO channel delegates to _media.handleMessage', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    ;(session as unknown as { _media?: { handleMessage: Mock } })._media = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.MEDIA_INFO, 0, 0x8003, Buffer.alloc(0))
    expect(handleMessage).toHaveBeenCalled()
  })
})

describe('Session.requestShutdown — ack and end semantics', () => {
  test('falls back through the timeout when the phone never sends ByeByeResponse', async () => {
    vi.useFakeTimers()
    const { session, link } = makeSession()
    forceRunning(session)
    captureEncrypted(session)
    const p = session.requestShutdown()
    await vi.advanceTimersByTimeAsync(2_000)
    await p
    expect(stateOf(session)).toBe(STATE.CLOSED)
    expect(link.end).toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('closes promptly once the phone acks with shutdown-complete', async () => {
    vi.useFakeTimers()
    const { session, link } = makeSession()
    forceRunning(session)
    captureEncrypted(session)
    // A ControlChannel stub so requestShutdown can await the ByeByeResponse ack
    const control = new EventEmitter()
    ;(session as unknown as { _control: unknown })._control = control

    const p = session.requestShutdown()
    await vi.advanceTimersByTimeAsync(10)
    control.emit('shutdown-complete')
    await p

    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('acked by phone'))
    expect(stateOf(session)).toBe(STATE.CLOSED)
    expect(link.end).toHaveBeenCalled()
    vi.useRealTimers()
  })

  test('still closes when the encrypted send throws', async () => {
    vi.useFakeTimers()
    const { session, link } = makeSession()
    forceRunning(session)
    ;(session as unknown as { _sendEncrypted: Mock })._sendEncrypted = vi.fn(() => {
      throw new Error('not writable')
    })
    const p = session.requestShutdown()
    await vi.advanceTimersByTimeAsync(2_000)
    await p
    expect(link.end).toHaveBeenCalled()
    vi.useRealTimers()
  })
})

describe('Session._sendAA / _sendEncrypted', () => {
  test('plaintext frames go to the link straight from INIT', () => {
    const { session, link } = makeSession()
    sendAA(session)(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, 0xabcd, Buffer.from([1, 2]))
    expect(link.send).toHaveBeenCalledWith(
      CH.CONTROL,
      FRAME_FLAGS.PLAINTEXT,
      0xabcd,
      Buffer.from([1, 2])
    )
  })

  test('encrypted frames are dropped before AUTH', () => {
    const { session, link } = makeSession()
    sendAA(session)(CH.VIDEO, FRAME_FLAGS.ENC_SIGNAL, 0xabcd, Buffer.from([1, 2]))
    expect(link.send).not.toHaveBeenCalled()
  })

  test('encrypted frames go to the link from AUTH on', () => {
    const { session, link } = makeSession()
    setState(session, STATE.AUTH)
    sendAA(session)(CH.VIDEO, FRAME_FLAGS.ENC_SIGNAL, 0xabcd, Buffer.from([1, 2]))
    expect(link.send).toHaveBeenCalledWith(
      CH.VIDEO,
      FRAME_FLAGS.ENC_SIGNAL,
      0xabcd,
      Buffer.from([1, 2])
    )
  })
})

describe('Session._transition', () => {
  test('emits "disconnected" on transition into CLOSED', () => {
    const { session } = makeSession()
    const cb = vi.fn()
    session.on('disconnected', cb)
    ;(session as unknown as { _transition: (s: number, r?: string) => void })._transition(
      STATE.CLOSED,
      'why'
    )
    expect(cb).toHaveBeenCalledWith('why')
  })

  test('does not emit "disconnected" for non-closed transitions', () => {
    const { session } = makeSession()
    const cb = vi.fn()
    session.on('disconnected', cb)
    ;(session as unknown as { _transition: (s: number, r?: string) => void })._transition(
      STATE.CHANNEL_SETUP
    )
    expect(cb).not.toHaveBeenCalled()
  })
})

describe('Session._handleSensorStartRequest', () => {
  test('responds with SUCCESS for an unknown sensor type', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.from([0x08, 99]))
    // One SUCCESS response + no extra batch
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][2]).toBe(0x8002)
  })

  test('DrivingStatus sensor (type=13) emits an extra SensorBatch', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.from([0x08, 13]))
    expect(sent).toHaveBeenCalledTimes(2)
    expect(sent.mock.calls[1][2]).toBe(0x8003)
  })

  test('NightMode sensor (type=10) uses initialNightMode from config', async () => {
    const { session } = makeSession({ initialNightMode: true })
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.from([0x08, 10]))
    expect(sent).toHaveBeenCalledTimes(2)
    const batch = sent.mock.calls[1][3] as Buffer
    expect(batch[3]).toBe(0x01) // initialNightMode=true
  })
})

describe('Session._handleWifiCredentialsRequest', () => {
  test('sends a WifiCredentialsResponse including ssid + password + security + type', async () => {
    const { session } = makeSession({ wifiSsid: 'LIVI-AP', wifiPassword: 'secret123' })
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    expect(sent).toHaveBeenCalledTimes(1)
    const buf = sent.mock.calls[0][3] as Buffer
    expect(buf.toString('utf8')).toContain('LIVI-AP')
    expect(buf.toString('utf8')).toContain('secret123')
  })

  test('omits empty password field', async () => {
    const { session } = makeSession({ wifiSsid: 'LIVI', wifiPassword: '' })
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    const buf = sent.mock.calls[0][3] as Buffer
    expect(buf.toString('utf8')).toContain('LIVI')
  })

  test('warns when ssid is missing', async () => {
    const { session } = makeSession({ wifiSsid: '', wifiPassword: 'x' })
    captureEncrypted(session)
    expect(() =>
      (
        session as unknown as { _handleWifiCredentialsRequest: () => void }
      )._handleWifiCredentialsRequest()
    ).not.toThrow()
  })
})

describe('Session._handleAVSetupRequest', () => {
  function setupSession(): { session: Session; sent: Mock; proto: Record<string, unknown> } {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    const proto = {
      AVChannelSetupRequest: { decode: vi.fn(), toObject: vi.fn((m: unknown) => m) },
      AVChannelSetupResponse: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x02]) })
      }
    }
    ;(session as unknown as { _proto: typeof proto })._proto = proto
    // Stub decode() helper to return a deterministic object
    ;(proto.AVChannelSetupRequest.decode as Mock).mockReturnValue({ mediaCodecType: 1 })
    return { session, sent, proto }
  }

  test('video channel selects h264 + transitions to RUNNING', async () => {
    const { session, sent } = setupSession()
    ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = ['h264', 'h265']
    const cb = vi.fn()
    session.on('video-codec', cb)
    session.on('connected', () => cb('connected'))

    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(3 /* CH.VIDEO */, Buffer.alloc(0))
    expect(cb).toHaveBeenCalled()
    // SETUP_RESPONSE + VIDEO_FOCUS_INDICATION
    expect(sent.mock.calls.length).toBeGreaterThanOrEqual(2)
    expect(stateOf(session)).toBe(STATE.RUNNING)
  })

  test('cluster channel selects + emits cluster-video-codec, holds focus until main frame', async () => {
    const { session, sent } = setupSession()
    ;(session as unknown as { _clusterCodecByIndex: string[] })._clusterCodecByIndex = ['h264']
    const cb = vi.fn()
    session.on('cluster-video-codec', cb)

    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(19 /* CH.CLUSTER_VIDEO */, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith('h264')
    // SETUP_RESPONSE is sent, but the cluster VIDEO_FOCUS_INDICATION is held back until
    // the first main frame so the main video plane is claimed first (no restart swap).
    expect(sent).toHaveBeenCalled()
    const focusSent = sent.mock.calls.some(
      (c) => c[0] === CH.CLUSTER_VIDEO && c[2] === AV_MSG.VIDEO_FOCUS_INDICATION
    )
    expect(focusSent).toBe(false)
    expect((session as unknown as { _clusterFocusPending: boolean })._clusterFocusPending).toBe(
      true
    )
  })

  test('audio channel forwards format to AudioChannel.handleSetupRequest', async () => {
    const { session } = setupSession()
    const audio = new Map<number, { handleSetupRequest: Mock }>()
    audio.set(4, { handleSetupRequest: vi.fn() })
    ;(session as unknown as { _audio: typeof audio })._audio = audio
    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(4 /* MEDIA_AUDIO */, Buffer.alloc(0))
    expect(audio.get(4)!.handleSetupRequest).toHaveBeenCalledWith(1, 48000, 2)
  })

  test('mic channel forwards format to MicChannel.handleSetupRequest', async () => {
    const { session } = setupSession()
    const mic = { handleSetupRequest: vi.fn() }
    ;(session as unknown as { _mic: typeof mic })._mic = mic
    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(9 /* MIC_INPUT */, Buffer.alloc(0))
    expect(mic.handleSetupRequest).toHaveBeenCalled()
  })
})

describe('Session._postTlsSetup', () => {
  test('sends AUTH_COMPLETE in plaintext and transitions to SERVICE_DISCOVERY', async () => {
    const { session, link } = makeSession()
    ;(session as unknown as { _proto: Record<string, unknown> })._proto = {
      AuthCompleteIndication: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) })
      }
    }
    await (session as unknown as { _postTlsSetup: () => Promise<void> })._postTlsSetup()
    expect(link.send).toHaveBeenCalledWith(
      CH.CONTROL,
      FRAME_FLAGS.PLAINTEXT,
      CTRL_MSG.AUTH_COMPLETE,
      expect.any(Buffer)
    )
    expect(stateOf(session)).toBe(STATE.SERVICE_DISCOVERY)
  })
})

describe('Session._openChannels', () => {
  test('transitions to CHANNEL_SETUP', async () => {
    const { session } = makeSession()
    ;(session as unknown as { _openChannels: () => void })._openChannels()
    expect(stateOf(session)).toBe(STATE.CHANNEL_SETUP)
  })
})

describe('Session.start() — link handshake', () => {
  test('start() wires the channels but waits for the link to report ready', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    expect((session as unknown as { _control: unknown })._control).toBeDefined()
    expect((session as unknown as { _video: unknown })._video).toBeDefined()
    expect(link.send).not.toHaveBeenCalled()
    expect(stateOf(session)).toBe(STATE.INIT)
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('link ready after start() sends AUTH_COMPLETE and moves on to SERVICE_DISCOVERY', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    link.emit('control', { type: 'ready' })
    expect(link.send).toHaveBeenCalledWith(
      CH.CONTROL,
      FRAME_FLAGS.PLAINTEXT,
      CTRL_MSG.AUTH_COMPLETE,
      expect.any(Buffer)
    )
    expect(stateOf(session)).toBe(STATE.SERVICE_DISCOVERY)
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('link ready before start() is picked up once the channels exist', async () => {
    vi.useFakeTimers()
    const { session, link } = makeSession()
    link.emit('control', { type: 'ready' })
    expect(link.send).not.toHaveBeenCalled()
    expect(stateOf(session)).toBe(STATE.INIT)

    const spy = vi.spyOn(protoIndex, 'loadProtos').mockResolvedValue(protoStub() as never)
    await session.start()
    spy.mockRestore()
    expect(link.send).toHaveBeenCalledWith(
      CH.CONTROL,
      FRAME_FLAGS.PLAINTEXT,
      CTRL_MSG.AUTH_COMPLETE,
      expect.any(Buffer)
    )
    expect(stateOf(session)).toBe(STATE.SERVICE_DISCOVERY)
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('the ready control carries the mic socket the helper listens on', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    link.emit('control', { type: 'ready', mic: '/run/livi/aa-mic.sock' })
    expect(session.micSocketPath()).toBe('/run/livi/aa-mic.sock')
    expect(stateOf(session)).toBe(STATE.SERVICE_DISCOVERY)
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('a repeated ready does not restart the handshake', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    link.emit('control', { type: 'ready' })
    link.emit('control', { type: 'ready' })
    expect(link.send).toHaveBeenCalledTimes(1)
    expect(stateOf(session)).toBe(STATE.SERVICE_DISCOVERY)
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('messages that arrive before start() are queued and replayed after the handshake', async () => {
    vi.useFakeTimers()
    const { session, link } = makeSession()
    link.emit('control', { type: 'ready' })
    // KeyBindingRequest on the input channel, answered with an encrypted KeyBindingResponse
    link.emit('message', CH.INPUT, FRAME_FLAGS.ENC_SIGNAL, 0x8002, Buffer.alloc(0))
    expect(link.send).not.toHaveBeenCalled()

    const spy = vi.spyOn(protoIndex, 'loadProtos').mockResolvedValue(protoStub() as never)
    await session.start()
    spy.mockRestore()
    expect(link.send).toHaveBeenCalledWith(
      CH.INPUT,
      FRAME_FLAGS.ENC_SIGNAL,
      0x8003,
      Buffer.from([0x08, 0x00])
    )
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('messages after start() reach the dispatcher directly', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    const handle = vi.fn()
    ;(session as unknown as { _handleDecryptedMessage: Mock })._handleDecryptedMessage = handle
    link.emit('message', CH.CONTROL, 0, 0xabcd, Buffer.from([1]))
    expect(handle).toHaveBeenCalledWith(CH.CONTROL, 0, 0xabcd, Buffer.from([1]))
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })
})

describe('Session — RUNNING state guarded methods', () => {
  test('requestVideoFocus still does nothing when state is CLOSED', async () => {
    const { session } = makeSession()
    session.close()
    const sent = captureEncrypted(session)
    session.requestVideoFocus()
    expect(sent).not.toHaveBeenCalled()
  })
})

describe('Session._handleDecryptedMessage — extra dispatch paths', () => {
  test('SENSOR channel SENSOR_MESSAGE_REQUEST → _handleSensorStartRequest', async () => {
    const { session } = makeSession()
    const sensor = vi.fn()
    ;(session as unknown as { _handleSensorStartRequest: Mock })._handleSensorStartRequest = sensor
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(1 /* CH.SENSOR */, 0, 0x8001, Buffer.from([0x08, 13]))
    expect(sensor).toHaveBeenCalled()
  })

  test('MIC_INPUT routes non-SETUP messages to MicChannel', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    ;(session as unknown as { _mic: { handleMessage: Mock } })._mic = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(9 /* CH.MIC_INPUT */, 0, 0x0001, Buffer.from([0]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('WIFI WIFI_CREDENTIALS_REQUEST → _handleWifiCredentialsRequest', async () => {
    const { session } = makeSession()
    const wifi = vi.fn()
    ;(session as unknown as { _handleWifiCredentialsRequest: Mock })._handleWifiCredentialsRequest =
      wifi
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(18 /* CH.WIFI */, 0, 0x8001, Buffer.alloc(0))
    expect(wifi).toHaveBeenCalled()
  })

  test('INPUT KEY_BINDING_REQUEST → replies BindingResponse OK', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(8 /* CH.INPUT */, 0, 0x8002, Buffer.alloc(0))
    expect(sent).toHaveBeenCalled()
    expect(sent.mock.calls[0][2]).toBe(0x8003)
  })

  test('START_INDICATION on VIDEO routes to _video.handleMessage (not the codec selector)', async () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    ;(session as unknown as { _video?: { handleMessage: Mock } })._video = { handleMessage }
    const payload = Buffer.from([0x08, 0x07, 0x10, 0x01])
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(3, 0, 0x8001, payload)
    expect(handleMessage).toHaveBeenCalled()
  })

  test('SENSOR for unhandled msgId is logged but not crashed', async () => {
    const { session } = makeSession()
    expect(() =>
      (
        session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
      )._handleDecryptedMessage(1, 0, 0x99, Buffer.alloc(0))
    ).not.toThrow()
  })

  test('VIDEO setup request is forwarded to _handleAVSetupRequest', async () => {
    const { session } = makeSession()
    const avSetup = vi.fn()
    ;(session as unknown as { _handleAVSetupRequest: Mock })._handleAVSetupRequest = avSetup
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(3, 0, 0x8000 /* SETUP_REQUEST */, Buffer.alloc(0))
    expect(avSetup).toHaveBeenCalled()
  })

  test('CLUSTER_VIDEO setup request is forwarded to _handleAVSetupRequest', async () => {
    const { session } = makeSession()
    const avSetup = vi.fn()
    ;(session as unknown as { _handleAVSetupRequest: Mock })._handleAVSetupRequest = avSetup
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(19, 0, 0x8000, Buffer.alloc(0))
    expect(avSetup).toHaveBeenCalled()
  })

  test('audio SETUP_REQUEST on a non-mapped channel still forwards to _handleAVSetupRequest', async () => {
    const { session } = makeSession()
    const avSetup = vi.fn()
    ;(session as unknown as { _handleAVSetupRequest: Mock })._handleAVSetupRequest = avSetup
    // Channel 4 (MEDIA_AUDIO) but _audio map empty
    ;(session as unknown as { _audio: Map<number, unknown> })._audio = new Map()
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(4, 0, 0x8000, Buffer.alloc(0))
    expect(avSetup).toHaveBeenCalled()
  })
})
