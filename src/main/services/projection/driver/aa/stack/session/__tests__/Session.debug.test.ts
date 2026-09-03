import { EventEmitter } from 'node:events'
import { setDebugLogging } from '@main/constants'
import type { Mock } from 'vitest'
import type { HelperSessionLink } from '../../transport/HelperSessionLink'

// Stands in for the helper session socket
class FakeLink extends EventEmitter {
  peer = '::ffff:10.0.0.9'
  closed = false
  send = vi.fn()
  control = vi.fn()
  end = vi.fn()
  destroy = vi.fn()
}

vi.mock('../ServiceDiscoveryBuilder', () => ({
  buildServiceDiscoveryResponse: vi.fn(() => ({
    buf: Buffer.from([0x08, 0x00]),
    videoCodecByIndex: ['h264', 'h265'],
    clusterCodecByIndex: ['h264']
  }))
}))

const ORIG_DEBUG = process.env.DEBUG
const ORIG_TRACE = process.env.TRACE

// State enum in Session.ts: INIT, AUTH, SERVICE_DISCOVERY, CHANNEL_SETUP, RUNNING, CLOSED
const CHANNEL_SETUP = 3
const RUNNING = 4

type SessionModule = typeof import('../Session')
type ConstModule = typeof import('../../constants')
type ProtoModule = typeof import('../../proto/index')

let Session: SessionModule['Session']
let C: ConstModule
let proto: ProtoModule

beforeAll(async () => {
  process.env.DEBUG = '1'
  process.env.TRACE = '1'
  vi.resetModules()
  ;({ Session } = await import('../Session'))
  C = await import('../../constants')
  proto = await import('../../proto/index')
})

afterAll(() => {
  if (ORIG_DEBUG === undefined) delete process.env.DEBUG
  else process.env.DEBUG = ORIG_DEBUG
  if (ORIG_TRACE === undefined) delete process.env.TRACE
  else process.env.TRACE = ORIG_TRACE
  vi.resetModules()
})

beforeEach(() => {
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => vi.restoreAllMocks())

function protoStub(): Record<string, unknown> {
  const codec = {
    verify: () => null,
    create: (f: Record<string, unknown>) => f,
    encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) }),
    decode: () => ({ mediaCodecType: 1, signalStrength: 3 }),
    toObject: (m: unknown) => m
  }
  return {
    ChannelOpenResponse: codec,
    AVChannelSetupRequest: codec,
    AVChannelSetupResponse: codec,
    AuthCompleteIndication: codec,
    ServiceDiscoveryResponse: codec,
    PingRequest: codec,
    PhoneStatus: codec,
    BluetoothPairingRequest: codec,
    BluetoothPairingResponse: codec
  }
}

function cfg(over: Record<string, unknown> = {}): import('../Session').SessionConfig {
  return {
    huName: 'LIVI',
    videoWidth: 1280,
    videoHeight: 720,
    clusterEnabled: false,
    clusterWidth: 0,
    clusterHeight: 0,
    clusterFps: 0,
    clusterDpi: 0,
    ...over
  } as import('../Session').SessionConfig
}

function make(over: Record<string, unknown> = {}): {
  session: InstanceType<SessionModule['Session']>
  link: FakeLink
} {
  const link = new FakeLink()
  const session = new Session(link as unknown as HelperSessionLink, cfg(over))
  return { session, link }
}

function run(session: unknown): (ch: number, fl: number, mid: number, p: Buffer) => void {
  return (
    session as { _handleDecryptedMessage: (...a: unknown[]) => void }
  )._handleDecryptedMessage.bind(session)
}

function capture(session: unknown): Mock {
  const fn = vi.fn()
  ;(session as { _sendEncrypted: Mock })._sendEncrypted = fn
  return fn
}

describe('Session under DEBUG + TRACE', () => {
  test('decrypted dispatch logs across channels', () => {
    const { session } = make()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    capture(session)
    const d = run(session)
    d(C.CH.VIDEO, 0, 0x0001, Buffer.from([1, 2]))
    d(C.CH.CONTROL, 0, C.CTRL_MSG.PING_REQUEST, Buffer.alloc(0))
    d(C.CH.SENSOR, 0, 0x9999, Buffer.alloc(0))
    d(C.CH.WIFI, 0, 0x9999, Buffer.alloc(0))
    d(C.CH.INPUT, 0, 0x8002, Buffer.alloc(0))
    d(0x7e, 0, 0x9999, Buffer.alloc(0))
    d(C.CH.MEDIA_AUDIO, 0, C.AV_MSG.START_INDICATION, Buffer.from([0x08, 0x01]))
    d(C.CH.PHONE_STATUS, 0, 0x8001, Buffer.alloc(0))
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('[Session] MSG ch='))
  })

  test('CHANNEL_OPEN_REQUEST logs and responds', () => {
    const { session } = make()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    const sent = capture(session)
    run(session)(C.CH.VIDEO, 0, C.CTRL_MSG.CHANNEL_OPEN_REQUEST, Buffer.alloc(0))
    expect(sent).toHaveBeenCalled()
  })

  test('AVSetupRequest logs for video, cluster, audio and mic', () => {
    const mkSetup = (chId: number, over: Record<string, unknown> = {}): void => {
      const { session } = make(over)
      const p = protoStub()
      ;(p.AVChannelSetupRequest as { decode: Mock }).decode = vi.fn(() => ({ mediaCodecType: 1 }))
      ;(session as unknown as { _proto: unknown })._proto = p
      ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = ['h264']
      ;(session as unknown as { _clusterCodecByIndex: string[] })._clusterCodecByIndex = ['h264']
      capture(session)
      ;(
        session as unknown as { _handleAVSetupRequest: (c: number, x: Buffer) => void }
      )._handleAVSetupRequest(chId, Buffer.alloc(0))
    }
    mkSetup(C.CH.VIDEO)
    mkSetup(C.CH.CLUSTER_VIDEO)
    const withMic = make()
    ;(withMic.session as unknown as { _mic: unknown })._mic = { handleSetupRequest: vi.fn() }
    const p = protoStub()
    ;(p.AVChannelSetupRequest as { decode: Mock }).decode = vi.fn(() => ({ mediaCodecType: 1 }))
    ;(withMic.session as unknown as { _proto: unknown })._proto = p
    capture(withMic.session)
    ;(
      withMic.session as unknown as { _handleAVSetupRequest: (c: number, x: Buffer) => void }
    )._handleAVSetupRequest(C.CH.MIC_INPUT, Buffer.alloc(0))
  })

  test('sensor start request logs for driving-status and night-mode (both defaults)', () => {
    for (const [type, night] of [
      [13, true],
      [10, true],
      [10, false],
      [99, false]
    ] as const) {
      const { session } = make({ initialNightMode: night })
      capture(session)
      ;(
        session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
      )._handleSensorStartRequest(Buffer.from([0x08, type]))
    }
  })

  test('MSG log renders an out-of-range numeric state', () => {
    const { session } = make()
    ;(session as unknown as { _state: number })._state = 99
    ;(session as unknown as { _nav: unknown })._nav = { handleMessage: vi.fn() }
    run(session)(C.CH.NAVIGATION, 0, 0x8001, Buffer.alloc(0))
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('state=99'))
  })

  test('video-ready log falls back to h264 when no codec is set', () => {
    const { session } = make()
    const p = protoStub()
    ;(p.AVChannelSetupRequest as { decode: Mock }).decode = vi.fn(() => ({ mediaCodecType: 1 }))
    ;(session as unknown as { _proto: unknown })._proto = p
    ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = ['h264']
    ;(session as unknown as { _videoCodec: unknown })._videoCodec = null
    ;(session as unknown as { _phoneCodecLogged: boolean })._phoneCodecLogged = true
    capture(session)
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, x: Buffer) => void }
    )._handleAVSetupRequest(C.CH.VIDEO, Buffer.alloc(0))
  })

  test('wifi credentials response logs, including the missing-ssid warning', () => {
    const ok = make({ wifiSsid: 'AP', wifiPassword: 'pw' })
    capture(ok.session)
    ;(
      ok.session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    const noSsid = make({ wifiSsid: '', wifiPassword: 'pw' })
    capture(noSsid.session)
    ;(
      noSsid.session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('no wifiSsid configured'))
  })

  test('post-TLS setup and channel setup log', async () => {
    const { session, link } = make()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    await (session as unknown as { _postTlsSetup: () => Promise<void> })._postTlsSetup()
    expect(link.send).toHaveBeenCalledWith(
      C.CH.CONTROL,
      C.FRAME_FLAGS.PLAINTEXT,
      C.CTRL_MSG.AUTH_COMPLETE,
      expect.any(Buffer)
    )
    ;(session as unknown as { _openChannels: () => void })._openChannels()
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('AUTH_COMPLETE sent'))
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Channel setup'))
  })

  test('a running main video focus request logs under DEBUG', () => {
    const { session } = make()
    capture(session)
    ;(session as unknown as { _state: number })._state = 4
    ;(session as unknown as { requestVideoFocus: () => void }).requestVideoFocus()
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('main video focus request'))
  })

  test('start(), link ready, SDR, ping and shutdown logging', async () => {
    vi.useFakeTimers()
    const { session, link } = make()
    const spy = vi.spyOn(proto, 'loadProtos').mockResolvedValue(protoStub() as never)
    await session.start()
    spy.mockRestore()
    link.emit('control', { type: 'ready' })
    expect(link.send).toHaveBeenCalledWith(
      C.CH.CONTROL,
      C.FRAME_FLAGS.PLAINTEXT,
      C.CTRL_MSG.AUTH_COMPLETE,
      expect.any(Buffer)
    )
    const control = (session as unknown as { _control: EventEmitter })._control
    ;(session as unknown as { _sendAA: Mock })._sendAA = vi.fn()
    control.emit('service-discovery-request', {
      deviceName: 'P',
      deviceBrand: 'B',
      phoneInfo: { instanceId: 'i' }
    })
    control.emit('service-discovery-request', {})
    control.emit('service-discovery-request', { deviceName: 42, phoneInfo: { instanceId: 7 } })
    control.emit('shutdown', 2)
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('SDR + Ping sent'))
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Phone shutdown, reason=2'))
    ;(session as unknown as { close: (r?: string) => void }).close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('VEM, shutdown request and cluster stream logs', async () => {
    vi.useFakeTimers()
    const { session } = make()
    ;(session as unknown as { _state: number })._state = RUNNING
    ;(session as unknown as { _mainFrameSeen: boolean })._mainFrameSeen = true
    capture(session)
    session.sendVehicleEnergyModel(50_000, 30_000, 200_000)
    ;(session as unknown as { _requestClusterStream: () => void })._requestClusterStream()
    ;(session as unknown as { _stopClusterStream: () => void })._stopClusterStream()
    ;(session as unknown as { _state: number })._state = CHANNEL_SETUP
    ;(session as unknown as { _mainFrameSeen: boolean })._mainFrameSeen = false
    ;(session as unknown as { _requestClusterStream: () => void })._requestClusterStream()
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('held until first main frame'))
    ;(session as unknown as { _state: number })._state = RUNNING
    const p = session.requestShutdown()
    await vi.advanceTimersByTimeAsync(2000)
    await p
    expect(console.log).toHaveBeenCalledWith(
      expect.stringContaining('requesting shutdown reason=1')
    )
    vi.useRealTimers()
  })

  test('requestShutdown logs when the encrypted send throws', async () => {
    vi.useFakeTimers()
    const { session } = make()
    ;(session as unknown as { _state: number })._state = RUNNING
    ;(session as unknown as { _sendEncrypted: Mock })._sendEncrypted = vi.fn(() => {
      throw new Error('closed')
    })
    const p = session.requestShutdown()
    await vi.advanceTimersByTimeAsync(2000)
    await p
    expect(console.warn).toHaveBeenCalledWith(expect.stringContaining('shutdown send failed'))
    vi.useRealTimers()
  })

  test('CH.BLUETOOTH pairing request is answered with an already-paired response', () => {
    const { session } = make()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    const sendAA = vi.fn()
    ;(session as unknown as { _sendAA: Mock })._sendAA = sendAA
    run(session)(C.CH.BLUETOOTH, 0, 0x8001, Buffer.alloc(0))
    expect(sendAA).toHaveBeenCalledWith(
      C.CH.BLUETOOTH,
      expect.any(Number),
      0x8002,
      expect.anything()
    )
  })

  test('CH.BLUETOOTH ignores non-pairing message ids', () => {
    const { session } = make()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    const sendAA = vi.fn()
    ;(session as unknown as { _sendAA: Mock })._sendAA = sendAA
    run(session)(C.CH.BLUETOOTH, 0, 0x9999, Buffer.alloc(0))
    expect(sendAA).not.toHaveBeenCalled()
  })

  test('CH.BLUETOOTH pairing still answers when the request fails to decode', () => {
    setDebugLogging(true)
    const { session } = make()
    const proto = protoStub()
    ;(proto.BluetoothPairingRequest as { decode: unknown }).decode = () => {
      throw new Error('bad payload')
    }
    ;(session as unknown as { _proto: unknown })._proto = proto
    const sendAA = vi.fn()
    ;(session as unknown as { _sendAA: Mock })._sendAA = sendAA
    run(session)(C.CH.BLUETOOTH, 0, 0x8001, Buffer.alloc(0))
    expect(sendAA).toHaveBeenCalledWith(
      C.CH.BLUETOOTH,
      expect.any(Number),
      0x8002,
      expect.anything()
    )
  })
})
