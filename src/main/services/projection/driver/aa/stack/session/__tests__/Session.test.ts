import { EventEmitter } from 'node:events'

class MockSocket extends EventEmitter {
  destroy = jest.fn()
  end = jest.fn()
  setKeepAlive = jest.fn()
  write = jest.fn((_data: Buffer, cb?: () => void) => {
    cb?.()
    return true
  })
}

import { CH, CTRL_MSG, FRAME_FLAGS } from '../../constants'
import { Session, type SessionConfig } from '../Session'

const RUNNING_STATE = 6

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

function makeSession(): { session: Session; sock: MockSocket } {
  const sock = new MockSocket()
  const session = new Session(sock as unknown as import('net').Socket, baseCfg())
  return { session, sock }
}

function forceRunning(session: Session): void {
  ;(session as unknown as { _state: number })._state = RUNNING_STATE
}

function captureEncrypted(session: Session): jest.Mock {
  const fn = jest.fn()
  ;(session as unknown as { _sendEncrypted: jest.Mock })._sendEncrypted = fn
  return fn
}

beforeEach(() => {
  jest.spyOn(console, 'log').mockImplementation(() => {})
  jest.spyOn(console, 'warn').mockImplementation(() => {})
  jest.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => jest.restoreAllMocks())

describe('Session — construction', () => {
  test('sets TCP keepalive on the socket', () => {
    const { sock } = makeSession()
    expect(sock.setKeepAlive).toHaveBeenCalledWith(true, 5_000)
  })

  test('keepalive failure is swallowed', () => {
    const sock = new MockSocket()
    sock.setKeepAlive = jest.fn(() => {
      throw new Error('not a real socket')
    })
    expect(() => new Session(sock as unknown as import('net').Socket, baseCfg())).not.toThrow()
  })
})

describe('Session.close', () => {
  test('destroys the socket and transitions to CLOSED', () => {
    const { session, sock } = makeSession()
    session.close()
    expect(sock.destroy).toHaveBeenCalled()
    expect((session as unknown as { _state: number })._state).toBe(7) // CLOSED
  })

  test('is idempotent — second close is silent', () => {
    const { session, sock } = makeSession()
    session.close()
    sock.destroy.mockClear()
    session.close()
    expect(sock.destroy).toHaveBeenCalled() // still calls destroy
  })

  test('survives destroy() throwing', () => {
    const { session, sock } = makeSession()
    sock.destroy = jest.fn(() => {
      throw new Error('already destroyed')
    })
    expect(() => session.close()).not.toThrow()
  })
})

describe('Session — socket events', () => {
  test('socket "close" event transitions to CLOSED', () => {
    const { session, sock } = makeSession()
    sock.emit('close')
    expect((session as unknown as { _state: number })._state).toBe(7)
  })

  test('socket "error" event emits + transitions to CLOSED', () => {
    const { session, sock } = makeSession()
    const onError = jest.fn()
    session.on('error', onError)
    sock.emit('error', new Error('reset'))
    expect(onError).toHaveBeenCalledWith(new Error('reset'))
    expect((session as unknown as { _state: number })._state).toBe(7)
  })

  test('socket "end" event before RUNNING half-closes the socket', () => {
    const { session, sock } = makeSession()
    sock.emit('end')
    expect(sock.end).toHaveBeenCalled()
    // Make TypeScript happy
    void session
  })

  test('socket "end" event in RUNNING leaves the write side open', () => {
    const { session, sock } = makeSession()
    forceRunning(session)
    sock.emit('end')
    expect(sock.end).not.toHaveBeenCalled()
  })
})

describe('Session — outbound input API gated by state', () => {
  test('sendTouch is a no-op outside RUNNING', () => {
    const { session } = makeSession()
    const input = { sendTouch: jest.fn() }
    ;(session as unknown as { _input?: { sendTouch: jest.Mock } })._input = input
    session.sendTouch(0, [{ x: 0, y: 0, id: 0 }])
    expect(input.sendTouch).not.toHaveBeenCalled()
  })

  test('sendTouch delegates to InputChannel when RUNNING', () => {
    const { session } = makeSession()
    forceRunning(session)
    const input = { sendTouch: jest.fn() }
    ;(session as unknown as { _input: typeof input })._input = input
    const pointers = [{ x: 5, y: 5, id: 0 }]
    session.sendTouch(0, pointers, 1)
    expect(input.sendTouch).toHaveBeenCalledWith(0, pointers, 1)
  })

  test('sendButton delegates when RUNNING', () => {
    const { session } = makeSession()
    forceRunning(session)
    const input = { sendButton: jest.fn() }
    ;(session as unknown as { _input: typeof input })._input = input
    session.sendButton(3, true)
    expect(input.sendButton).toHaveBeenCalledWith(3, true)
  })

  test('sendRotary delegates when RUNNING', () => {
    const { session } = makeSession()
    forceRunning(session)
    const input = { sendRotary: jest.fn() }
    ;(session as unknown as { _input: typeof input })._input = input
    session.sendRotary(1)
    expect(input.sendRotary).toHaveBeenCalledWith(1)
  })

  test('sendMicPcm delegates to MicChannel when RUNNING', () => {
    const { session } = makeSession()
    forceRunning(session)
    const mic = { pushPcm: jest.fn() }
    ;(session as unknown as { _mic: typeof mic })._mic = mic
    session.sendMicPcm(Buffer.from([1, 2, 3]), 42n)
    expect(mic.pushPcm).toHaveBeenCalledWith(Buffer.from([1, 2, 3]), 42n)
  })

  test('sendMicPcm without _mic is a no-op even in RUNNING', () => {
    const { session } = makeSession()
    forceRunning(session)
    expect(() => session.sendMicPcm(Buffer.alloc(0))).not.toThrow()
  })

  test('requestKeyframe / requestClusterKeyframe are no-ops outside RUNNING', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    session.requestKeyframe()
    session.requestClusterKeyframe()
    expect(sent).not.toHaveBeenCalled()
  })

  test('requestKeyframe emits a VIDEO_FOCUS_INDICATION on the video channel', () => {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestKeyframe()
    expect(sent.mock.calls[0][0]).toBe(CH.VIDEO)
    expect(sent.mock.calls[0][1]).toBe(FRAME_FLAGS.ENC_SIGNAL)
    expect((sent.mock.calls[0][3] as Buffer).equals(Buffer.from([0x08, 0x01]))).toBe(true)
  })

  test('requestClusterKeyframe emits a VIDEO_FOCUS_INDICATION on the cluster channel', () => {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestClusterKeyframe()
    expect(sent.mock.calls[0][0]).toBe(CH.CLUSTER_VIDEO)
  })
})

describe('Session — sensor pushes', () => {
  function setup(): { session: Session; sent: jest.Mock } {
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    return { session, sent }
  }

  test('all sensor methods are no-ops outside RUNNING', () => {
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

  test('sendFuelData writes a SensorBatch on CH.SENSOR', () => {
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

  test('sendLightData with no args writes nothing', () => {
    const { session, sent } = setup()
    session.sendLightData()
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendEnvironmentData with no args writes nothing', () => {
    const { session, sent } = setup()
    session.sendEnvironmentData()
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendGpsLocationData encodes lat/lon × 1e7 + optional fields', () => {
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

  test('sendVehicleEnergyModel is a no-op when capacity/current/range are non-positive', () => {
    const { session, sent } = setup()
    session.sendVehicleEnergyModel(0, 0, 0)
    expect(sent).not.toHaveBeenCalled()
  })

  test('sendVehicleEnergyModel writes one SensorBatch frame with valid inputs', () => {
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

describe('Session — pre-TLS data dispatch (raw parser)', () => {
  test('passes raw socket data to the frame parser before TLS handshake', () => {
    const { session, sock } = makeSession()
    const push = jest.fn()
    ;(session as unknown as { _rawParser: { push: jest.Mock } })._rawParser.push = push
    sock.emit('data', Buffer.from([0xde, 0xad]))
    expect(push).toHaveBeenCalled()
  })

  test('routes raw VERSION_RESPONSE frames through _onVersionResponse', async () => {
    const { session } = makeSession()
    const onVer = jest.fn(async () => {})
    ;(session as unknown as { _onVersionResponse: jest.Mock })._onVersionResponse = onVer
    await (
      session as unknown as { _handleRawFrame: (f: unknown) => Promise<void> }
    )._handleRawFrame({
      channelId: 0,
      flags: 0,
      msgId: CTRL_MSG.VERSION_RESPONSE,
      payload: Buffer.from([0x00, 0x01]),
      rawPayload: Buffer.alloc(4)
    })
    expect(onVer).toHaveBeenCalled()
  })

  test('routes SSL_HANDSHAKE bytes through the TLS bridge', async () => {
    const { session } = makeSession()
    const injectBytes = jest.fn()
    ;(session as unknown as { _bridge?: { injectBytes: jest.Mock } })._bridge = { injectBytes }
    await (
      session as unknown as { _handleRawFrame: (f: unknown) => Promise<void> }
    )._handleRawFrame({
      channelId: 0,
      flags: 0,
      msgId: CTRL_MSG.SSL_HANDSHAKE,
      payload: Buffer.from([1, 2, 3]),
      rawPayload: Buffer.alloc(4)
    })
    expect(injectBytes).toHaveBeenCalledWith(Buffer.from([1, 2, 3]))
  })
})

describe('Session — post-TLS dispatch (control channel)', () => {
  test('CONTROL channel forwards to ControlChannel.handleMessage', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    ;(session as unknown as { _control?: { handleMessage: jest.Mock } })._control = {
      handleMessage
    }
    ;(
      session as unknown as {
        _handleDecryptedMessage: (...args: unknown[]) => void
      }
    )._handleDecryptedMessage(CH.CONTROL, 0, 0xabcd, Buffer.from([1]))
    expect(handleMessage).toHaveBeenCalledWith(0xabcd, Buffer.from([1]))
  })

  test('CHANNEL_OPEN_REQUEST on a service channel triggers an encrypted response', () => {
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

  test('VIDEO channel messages delegate to _video.handleMessage', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    ;(session as unknown as { _video?: { handleMessage: jest.Mock } })._video = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.VIDEO, 0, 0x0001, Buffer.from([1, 2]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('audio channel messages delegate to the matching AudioChannel instance', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    const audioMap = new Map<number, { handleMessage: jest.Mock }>()
    audioMap.set(CH.MEDIA_AUDIO, { handleMessage })
    ;(session as unknown as { _audio: Map<number, unknown> })._audio = audioMap
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.MEDIA_AUDIO, 0, 0x0001, Buffer.from([0]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('NAVIGATION channel delegates to _nav.handleMessage', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    ;(session as unknown as { _nav?: { handleMessage: jest.Mock } })._nav = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.NAVIGATION, 0, 0x8001, Buffer.alloc(0))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('MEDIA_INFO channel delegates to _media.handleMessage', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    ;(session as unknown as { _media?: { handleMessage: jest.Mock } })._media = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(CH.MEDIA_INFO, 0, 0x8003, Buffer.alloc(0))
    expect(handleMessage).toHaveBeenCalled()
  })
})

describe('Session — _stripHeaderAndInjectTls header parsing', () => {
  test('routes plaintext frames through _handleDecryptedMessage', () => {
    const { session } = makeSession()
    const handle = jest.fn()
    ;(session as unknown as { _handleDecryptedMessage: jest.Mock })._handleDecryptedMessage = handle

    // SHORT frame: [ch=3][flags=0x03 (FIRST|LAST)][payloadSize=4 BE][msgId=0x1234 BE][2-byte data]
    const buf = Buffer.alloc(4 + 2 + 2)
    buf.writeUInt8(3, 0)
    buf.writeUInt8(0x03, 1) // not encrypted
    buf.writeUInt16BE(4, 2)
    buf.writeUInt16BE(0x1234, 4)
    buf.writeUInt16BE(0x5678, 6)
    ;(
      session as unknown as { _stripHeaderAndInjectTls: (b: Buffer) => void }
    )._stripHeaderAndInjectTls(buf)
    expect(handle).toHaveBeenCalledWith(3, 0x03, 0x1234, expect.any(Buffer))
  })

  test('encrypted frames are pushed into the TLS bridge', () => {
    const { session } = makeSession()
    const injectBytes = jest.fn()
    ;(session as unknown as { _bridge?: { injectBytes: jest.Mock } })._bridge = { injectBytes }

    // bit 3 (0x08) set marks the frame as encrypted
    const buf = Buffer.alloc(4 + 2)
    buf.writeUInt8(3, 0)
    buf.writeUInt8(0x08 | 0x03, 1)
    buf.writeUInt16BE(2, 2)
    buf.writeUInt16BE(0xdead, 4)
    ;(
      session as unknown as { _stripHeaderAndInjectTls: (b: Buffer) => void }
    )._stripHeaderAndInjectTls(buf)
    expect(injectBytes).toHaveBeenCalled()
  })
})

describe('Session.requestShutdown — write-and-end semantics', () => {
  test('hits the timeout escape hatch when writeChain never resolves', async () => {
    jest.useFakeTimers()
    const { session, sock } = makeSession()
    forceRunning(session)
    captureEncrypted(session)
    // Replace writeChain with a never-resolving promise
    ;(session as unknown as { _writeChain: Promise<void> })._writeChain = new Promise(() => {})
    const p = session.requestShutdown()
    jest.advanceTimersByTime(600)
    await p
    expect((session as unknown as { _state: number })._state).toBe(7) // CLOSED
    expect(sock.end).toHaveBeenCalled()
    jest.useRealTimers()
  })

  test('still closes when the encrypted send throws', async () => {
    jest.useFakeTimers()
    const { session, sock } = makeSession()
    forceRunning(session)
    ;(session as unknown as { _sendEncrypted: jest.Mock })._sendEncrypted = jest.fn(() => {
      throw new Error('not writable')
    })
    const p = session.requestShutdown()
    jest.advanceTimersByTime(600)
    await p
    expect(sock.end).toHaveBeenCalled()
    jest.useRealTimers()
  })
})

describe('Session._sendAA / _sendEncrypted', () => {
  test('plaintext flags → write frame on socket', () => {
    const { session, sock } = makeSession()
    ;(session as unknown as { _sendAA: (...args: unknown[]) => void })._sendAA(
      0,
      /* FRAME_FLAGS.PLAINTEXT */ 0x03,
      0xabcd,
      Buffer.from([1, 2])
    )
    expect(sock.write).toHaveBeenCalled()
  })

  test('encrypted flags without TLS state warn and drop', () => {
    const { session, sock } = makeSession()
    ;(session as unknown as { _sendAA: (...args: unknown[]) => void })._sendAA(
      3,
      /* ENC_SIGNAL */ 0x0b,
      0xabcd,
      Buffer.from([1, 2])
    )
    // Should not have written anything because TLS is not set up
    expect(sock.write).not.toHaveBeenCalled()
  })
})

describe('Session._transition', () => {
  test('emits "disconnected" on transition into CLOSED', () => {
    const { session } = makeSession()
    const cb = jest.fn()
    session.on('disconnected', cb)
    ;(session as unknown as { _transition: (s: number, r?: string) => void })._transition(7, 'why')
    expect(cb).toHaveBeenCalledWith('why')
  })

  test('does not emit "disconnected" for non-closed transitions', () => {
    const { session } = makeSession()
    const cb = jest.fn()
    session.on('disconnected', cb)
    ;(session as unknown as { _transition: (s: number, r?: string) => void })._transition(3)
    expect(cb).not.toHaveBeenCalled()
  })
})

describe('Session._onVersionResponse', () => {
  test('short payload is ignored', async () => {
    const { session } = makeSession()
    const transition = jest.fn()
    ;(session as unknown as { _transition: jest.Mock })._transition = transition
    await (
      session as unknown as { _onVersionResponse: (b: Buffer) => Promise<void> }
    )._onVersionResponse(Buffer.alloc(2))
    expect(transition).not.toHaveBeenCalled()
  })

  test('version mismatch transitions to CLOSED', async () => {
    const { session } = makeSession()
    const transition = jest.fn()
    ;(session as unknown as { _transition: jest.Mock })._transition = transition
    const payload = Buffer.alloc(6)
    payload.writeUInt16BE(1, 0)
    payload.writeUInt16BE(0, 2)
    payload.writeUInt16BE(/* VERSION.STATUS_MISMATCH = */ 0xffff, 4)
    await (
      session as unknown as { _onVersionResponse: (b: Buffer) => Promise<void> }
    )._onVersionResponse(payload)
    expect(transition).toHaveBeenCalledWith(7, expect.stringContaining('version mismatch'))
  })

  test('happy path transitions to TLS_HANDSHAKE + invokes _startTls', async () => {
    const { session } = makeSession()
    const transition = jest.fn()
    const startTls = jest.fn(async () => undefined)
    ;(session as unknown as { _transition: jest.Mock })._transition = transition
    ;(session as unknown as { _startTls: jest.Mock })._startTls = startTls

    const payload = Buffer.alloc(6)
    payload.writeUInt16BE(1, 0)
    payload.writeUInt16BE(0, 2)
    payload.writeUInt16BE(0, 4) // status OK
    await (
      session as unknown as { _onVersionResponse: (b: Buffer) => Promise<void> }
    )._onVersionResponse(payload)
    expect(transition).toHaveBeenCalledWith(2) // TLS_HANDSHAKE
    expect(startTls).toHaveBeenCalled()
  })
})

describe('Session._sendVersionRequest', () => {
  test('writes a VERSION_REQUEST frame on the socket', () => {
    const { session, sock } = makeSession()
    ;(session as unknown as { _sendVersionRequest: () => void })._sendVersionRequest()
    expect(sock.write).toHaveBeenCalled()
  })
})

describe('Session._handleSensorStartRequest', () => {
  function setupForSensor(session: Session): jest.Mock {
    const sent = captureEncrypted(session)
    return sent
  }

  test('responds with SUCCESS for an unknown sensor type', () => {
    const { session } = makeSession()
    const sent = setupForSensor(session)
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.from([0x08, 99]))
    // One SUCCESS response + no extra batch
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][2]).toBe(0x8002)
  })

  test('DrivingStatus sensor (type=13) emits an extra SensorBatch', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.from([0x08, 13]))
    expect(sent).toHaveBeenCalledTimes(2)
    expect(sent.mock.calls[1][2]).toBe(0x8003)
  })

  test('NightMode sensor (type=10) uses initialNightMode from config', () => {
    const sock = new MockSocket()
    const cfg = baseCfg({ initialNightMode: true })
    const session = new Session(sock as unknown as import('net').Socket, cfg)
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
  test('sends a WifiCredentialsResponse including ssid + password + security + type', () => {
    const sock = new MockSocket()
    const cfg = baseCfg({ wifiSsid: 'LIVI-AP', wifiPassword: 'secret123' })
    const session = new Session(sock as unknown as import('net').Socket, cfg)
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    expect(sent).toHaveBeenCalledTimes(1)
    const buf = sent.mock.calls[0][3] as Buffer
    expect(buf.toString('utf8')).toContain('LIVI-AP')
    expect(buf.toString('utf8')).toContain('secret123')
  })

  test('omits empty password field', () => {
    const sock = new MockSocket()
    const cfg = baseCfg({ wifiSsid: 'LIVI', wifiPassword: '' })
    const session = new Session(sock as unknown as import('net').Socket, cfg)
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    const buf = sent.mock.calls[0][3] as Buffer
    expect(buf.toString('utf8')).toContain('LIVI')
  })

  test('warns when ssid is missing', () => {
    const sock = new MockSocket()
    const cfg = baseCfg({ wifiSsid: '', wifiPassword: 'x' })
    const session = new Session(sock as unknown as import('net').Socket, cfg)
    captureEncrypted(session)
    expect(() =>
      (
        session as unknown as { _handleWifiCredentialsRequest: () => void }
      )._handleWifiCredentialsRequest()
    ).not.toThrow()
  })
})

describe('Session._handleAVSetupRequest', () => {
  function setupSession(): { session: Session; sent: jest.Mock; proto: Record<string, unknown> } {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    const proto = {
      AVChannelSetupRequest: { decode: jest.fn(), toObject: jest.fn((m: unknown) => m) },
      AVChannelSetupResponse: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x02]) })
      }
    }
    ;(session as unknown as { _proto: typeof proto })._proto = proto
    // Stub decode() helper to return a deterministic object
    ;(proto.AVChannelSetupRequest.decode as jest.Mock).mockReturnValue({ mediaCodecType: 1 })
    return { session, sent, proto }
  }

  test('video channel selects h264 + transitions to RUNNING', () => {
    const { session, sent } = setupSession()
    ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = ['h264', 'h265']
    const cb = jest.fn()
    session.on('video-codec', cb)
    session.on('connected', () => cb('connected'))

    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(3 /* CH.VIDEO */, Buffer.alloc(0))
    expect(cb).toHaveBeenCalled()
    // SETUP_RESPONSE + VIDEO_FOCUS_INDICATION
    expect(sent.mock.calls.length).toBeGreaterThanOrEqual(2)
    expect((session as unknown as { _state: number })._state).toBe(6) // RUNNING
  })

  test('cluster channel selects + emits cluster-video-codec', () => {
    const { session, sent } = setupSession()
    ;(session as unknown as { _clusterCodecByIndex: string[] })._clusterCodecByIndex = ['h264']
    const cb = jest.fn()
    session.on('cluster-video-codec', cb)

    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(19 /* CH.CLUSTER_VIDEO */, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith('h264')
    // SETUP_RESPONSE + VIDEO_FOCUS_INDICATION on cluster
    expect(sent.mock.calls.length).toBeGreaterThanOrEqual(2)
  })

  test('audio channel forwards format to AudioChannel.handleSetupRequest', () => {
    const { session } = setupSession()
    const audio = new Map<number, { handleSetupRequest: jest.Mock }>()
    audio.set(4, { handleSetupRequest: jest.fn() })
    ;(session as unknown as { _audio: typeof audio })._audio = audio
    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(4 /* MEDIA_AUDIO */, Buffer.alloc(0))
    expect(audio.get(4)!.handleSetupRequest).toHaveBeenCalledWith(1, 48000, 2)
  })

  test('mic channel forwards format to MicChannel.handleSetupRequest', () => {
    const { session } = setupSession()
    const mic = { handleSetupRequest: jest.fn() }
    ;(session as unknown as { _mic: typeof mic })._mic = mic
    ;(
      session as unknown as { _handleAVSetupRequest: (chId: number, p: Buffer) => void }
    )._handleAVSetupRequest(9 /* MIC_INPUT */, Buffer.alloc(0))
    expect(mic.handleSetupRequest).toHaveBeenCalled()
  })
})

describe('Session._postTlsSetup', () => {
  test('sends AUTH_COMPLETE and transitions to SERVICE_DISCOVERY', async () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    const sendAA = jest.fn()
    ;(session as unknown as { _sendAA: jest.Mock })._sendAA = sendAA
    ;(session as unknown as { _proto: Record<string, unknown> })._proto = {
      AuthCompleteIndication: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) })
      }
    }
    void sent
    await (session as unknown as { _postTlsSetup: () => Promise<void> })._postTlsSetup()
    expect(sendAA).toHaveBeenCalled()
    expect((session as unknown as { _state: number })._state).toBe(4) // SERVICE_DISCOVERY
  })
})

describe('Session._openChannels', () => {
  test('transitions to CHANNEL_SETUP', () => {
    const { session } = makeSession()
    ;(session as unknown as { _openChannels: () => void })._openChannels()
    expect((session as unknown as { _state: number })._state).toBe(5) // CHANNEL_SETUP
  })
})

describe('Session.start() — channel wiring', () => {
  function patchProto(session: Session): void {
    ;(session as unknown as { _proto: Record<string, unknown> })._proto = {
      ServiceDiscoveryResponse: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) })
      },
      AVChannelSetupRequest: {},
      AVChannelSetupResponse: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x02]) })
      },
      ChannelOpenResponse: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) })
      },
      AuthCompleteIndication: {
        verify: () => null,
        create: (f: Record<string, unknown>) => f,
        encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) })
      }
    }
  }

  test('start() wires every channel and sends VERSION_REQUEST', async () => {
    jest.useFakeTimers()
    const { session } = makeSession()
    const send = jest.fn()
    ;(session as unknown as { _sendVersionRequest: jest.Mock })._sendVersionRequest = send
    const loadProtosMod = jest.requireActual(
      '../../proto/index'
    ) as typeof import('../../proto/index')
    const spy = jest.spyOn(loadProtosMod, 'loadProtos').mockResolvedValue({} as never)
    patchProto(session)
    await session.start()
    expect(send).toHaveBeenCalled()
    spy.mockRestore()
    session.close()
    jest.clearAllTimers()
    jest.useRealTimers()
  })
})

describe('Session — RUNNING state guarded methods', () => {
  test('requestKeyframe still does nothing when state is CLOSED', () => {
    const { session } = makeSession()
    session.close()
    const sent = captureEncrypted(session)
    session.requestKeyframe()
    expect(sent).not.toHaveBeenCalled()
  })
})

describe('Session._stripHeaderAndInjectTls — EXTENDED frames', () => {
  test('parses an EXTENDED (FIRST-only) frame', () => {
    const { session } = makeSession()
    const handle = jest.fn()
    ;(session as unknown as { _handleDecryptedMessage: jest.Mock })._handleDecryptedMessage = handle

    // EXTENDED header is 8 bytes: ch + flags(FIRST) + payloadSize(2) + totalSize(4)
    const buf = Buffer.alloc(8 + 4)
    buf.writeUInt8(3, 0)
    buf.writeUInt8(0x01, 1) // FIRST only, not LAST → EXTENDED
    buf.writeUInt16BE(4, 2)
    buf.writeUInt32BE(8, 4) // totalSize
    buf.writeUInt16BE(0x1234, 8)
    buf.writeUInt16BE(0x5678, 10)
    ;(
      session as unknown as { _stripHeaderAndInjectTls: (b: Buffer) => void }
    )._stripHeaderAndInjectTls(buf)
    // FIRST-only plaintext is delivered to _handleDecryptedMessage too
    expect(handle).toHaveBeenCalled()
  })

  test('plaintext payload shorter than 2 bytes is logged + skipped', () => {
    const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
    const { session } = makeSession()
    const handle = jest.fn()
    ;(session as unknown as { _handleDecryptedMessage: jest.Mock })._handleDecryptedMessage = handle
    const buf = Buffer.alloc(4 + 1)
    buf.writeUInt8(3, 0)
    buf.writeUInt8(0x03, 1)
    buf.writeUInt16BE(1, 2)
    buf.writeUInt8(0xff, 4)
    ;(
      session as unknown as { _stripHeaderAndInjectTls: (b: Buffer) => void }
    )._stripHeaderAndInjectTls(buf)
    expect(handle).not.toHaveBeenCalled()
    warn.mockRestore()
  })
})

describe('Session._handleDecryptedMessage — extra dispatch paths', () => {
  test('SENSOR channel SENSOR_MESSAGE_REQUEST → _handleSensorStartRequest', () => {
    const { session } = makeSession()
    const sensor = jest.fn()
    ;(session as unknown as { _handleSensorStartRequest: jest.Mock })._handleSensorStartRequest =
      sensor
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(1 /* CH.SENSOR */, 0, 0x8001, Buffer.from([0x08, 13]))
    expect(sensor).toHaveBeenCalled()
  })

  test('MIC_INPUT routes non-SETUP messages to MicChannel', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    ;(session as unknown as { _mic: { handleMessage: jest.Mock } })._mic = { handleMessage }
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(9 /* CH.MIC_INPUT */, 0, 0x0001, Buffer.from([0]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('WIFI WIFI_CREDENTIALS_REQUEST → _handleWifiCredentialsRequest', () => {
    const { session } = makeSession()
    const wifi = jest.fn()
    ;(
      session as unknown as { _handleWifiCredentialsRequest: jest.Mock }
    )._handleWifiCredentialsRequest = wifi
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(18 /* CH.WIFI */, 0, 0x8001, Buffer.alloc(0))
    expect(wifi).toHaveBeenCalled()
  })

  test('INPUT KEY_BINDING_REQUEST → replies BindingResponse OK', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(8 /* CH.INPUT */, 0, 0x8002, Buffer.alloc(0))
    expect(sent).toHaveBeenCalled()
    expect(sent.mock.calls[0][2]).toBe(0x8003)
  })

  test('START_INDICATION on VIDEO routes to _video.handleMessage (not the codec selector)', () => {
    const { session } = makeSession()
    const handleMessage = jest.fn()
    ;(session as unknown as { _video?: { handleMessage: jest.Mock } })._video = { handleMessage }
    const payload = Buffer.from([0x08, 0x07, 0x10, 0x01])
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(3, 0, 0x8001, payload)
    expect(handleMessage).toHaveBeenCalled()
  })

  test('SENSOR for unhandled msgId is logged but not crashed', () => {
    const { session } = makeSession()
    expect(() =>
      (
        session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
      )._handleDecryptedMessage(1, 0, 0x99, Buffer.alloc(0))
    ).not.toThrow()
  })

  test('VIDEO setup request is forwarded to _handleAVSetupRequest', () => {
    const { session } = makeSession()
    const avSetup = jest.fn()
    ;(session as unknown as { _handleAVSetupRequest: jest.Mock })._handleAVSetupRequest = avSetup
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(3, 0, 0x8000 /* SETUP_REQUEST */, Buffer.alloc(0))
    expect(avSetup).toHaveBeenCalled()
  })

  test('CLUSTER_VIDEO setup request is forwarded to _handleAVSetupRequest', () => {
    const { session } = makeSession()
    const avSetup = jest.fn()
    ;(session as unknown as { _handleAVSetupRequest: jest.Mock })._handleAVSetupRequest = avSetup
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(19, 0, 0x8000, Buffer.alloc(0))
    expect(avSetup).toHaveBeenCalled()
  })

  test('audio SETUP_REQUEST on a non-mapped channel still forwards to _handleAVSetupRequest', () => {
    const { session } = makeSession()
    const avSetup = jest.fn()
    ;(session as unknown as { _handleAVSetupRequest: jest.Mock })._handleAVSetupRequest = avSetup
    // Channel 4 (MEDIA_AUDIO) but _audio map empty
    ;(session as unknown as { _audio: Map<number, unknown> })._audio = new Map()
    ;(
      session as unknown as { _handleDecryptedMessage: (...args: unknown[]) => void }
    )._handleDecryptedMessage(4, 0, 0x8000, Buffer.alloc(0))
    expect(avSetup).toHaveBeenCalled()
  })
})
