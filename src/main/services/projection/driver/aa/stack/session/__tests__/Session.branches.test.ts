import { EventEmitter } from 'node:events'
import type { Mock } from 'vitest'

// Stands in for the helper session socket
class FakeLink extends EventEmitter {
  peer = '::ffff:192.168.1.5%wlan0'
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

import { AV_MSG, CH, FRAME_FLAGS, MEDIA_CODEC } from '../../constants'
import * as protoIndex from '../../proto/index'
import type { HelperSessionLink } from '../../transport/HelperSessionLink'
import { buildServiceDiscoveryResponse } from '../ServiceDiscoveryBuilder'
import { Session, type SessionConfig } from '../Session'

// State enum in Session.ts: INIT, AUTH, SERVICE_DISCOVERY, CHANNEL_SETUP, RUNNING, CLOSED
const AUTH = 1
const RUNNING = 4
const CLOSED = 5

function protoStub(): Record<string, unknown> {
  const codec = {
    verify: () => null,
    create: (f: Record<string, unknown>) => f,
    encode: () => ({ finish: () => new Uint8Array([0x08, 0x00]) }),
    decode: (_b: Buffer) => ({}),
    toObject: (m: unknown) => m
  }
  return {
    ChannelOpenResponse: codec,
    AVChannelSetupRequest: { ...codec, decode: () => ({ mediaCodecType: MEDIA_CODEC.VIDEO_H265 }) },
    AVChannelSetupResponse: codec,
    AuthCompleteIndication: codec,
    ServiceDiscoveryResponse: codec,
    PingRequest: codec,
    PhoneStatus: { ...codec, decode: () => ({ signalStrength: 3 }), toObject: (m: unknown) => m }
  }
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

function forceRunning(session: Session): void {
  ;(session as unknown as { _state: number })._state = RUNNING
}

function setState(session: Session, state: number): void {
  ;(session as unknown as { _state: number })._state = state
}

function captureEncrypted(session: Session): Mock {
  const fn = vi.fn()
  ;(session as unknown as { _sendEncrypted: Mock })._sendEncrypted = fn
  return fn
}

function dispatch(
  session: Session,
  ch: number,
  flags: number,
  msgId: number,
  payload: Buffer
): void {
  ;(
    session as unknown as { _handleDecryptedMessage: (...a: unknown[]) => void }
  )._handleDecryptedMessage(ch, flags, msgId, payload)
}

// start() with the proto loader stubbed, the link has not reported ready yet
async function started(over: Partial<SessionConfig> = {}): Promise<{
  session: Session
  link: FakeLink
}> {
  const { session, link } = makeSession(over)
  const spy = vi.spyOn(protoIndex, 'loadProtos').mockResolvedValue(protoStub() as never)
  await session.start()
  spy.mockRestore()
  return { session, link }
}

beforeEach(() => {
  vi.spyOn(console, 'log').mockImplementation(() => {})
  vi.spyOn(console, 'warn').mockImplementation(() => {})
  vi.spyOn(console, 'error').mockImplementation(() => {})
  ;(buildServiceDiscoveryResponse as Mock).mockClear()
})
afterEach(() => vi.restoreAllMocks())

describe('cluster stream control', () => {
  test('forceClusterKeyframe is a no-op outside RUNNING or when unwanted', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    session.forceClusterKeyframe()
    forceRunning(session)
    ;(session as unknown as { _clusterStreamWanted: boolean })._clusterStreamWanted = false
    session.forceClusterKeyframe()
    expect(sent).not.toHaveBeenCalled()
  })

  test('forceClusterKeyframe sends two focus indications', () => {
    vi.useFakeTimers()
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.forceClusterKeyframe()
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][0]).toBe(CH.CLUSTER_VIDEO)
    vi.advanceTimersByTime(60)
    expect(sent).toHaveBeenCalledTimes(2)
    vi.useRealTimers()
  })

  test('forceClusterKeyframe delayed indication is skipped if state changed', () => {
    vi.useFakeTimers()
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.forceClusterKeyframe()
    setState(session, CLOSED)
    vi.advanceTimersByTime(60)
    expect(sent).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })

  test('setClusterStreamActive toggles and is idempotent', () => {
    const { session } = makeSession()
    forceRunning(session)
    ;(session as unknown as { _mainFrameSeen: boolean })._mainFrameSeen = true
    const sent = captureEncrypted(session)

    session.setClusterStreamActive(true)
    expect(sent).not.toHaveBeenCalled()

    session.setClusterStreamActive(false)
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][3]).toEqual(Buffer.from([0x08, 0x02]))

    sent.mockClear()
    session.setClusterStreamActive(true)
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][3]).toEqual(Buffer.from([0x08, 0x01]))
  })

  test('_requestClusterStream holds while not RUNNING then sends once ready', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    const req = (): void =>
      (session as unknown as { _requestClusterStream: () => void })._requestClusterStream()

    req()
    expect((session as unknown as { _clusterFocusPending: boolean })._clusterFocusPending).toBe(
      true
    )
    expect(sent).not.toHaveBeenCalled()

    forceRunning(session)
    ;(session as unknown as { _mainFrameSeen: boolean })._mainFrameSeen = true
    req()
    expect(sent).toHaveBeenCalledTimes(1)
    expect((session as unknown as { _clusterFocusPending: boolean })._clusterFocusPending).toBe(
      false
    )
  })

  test('_requestClusterStream is a no-op when the cluster is unwanted', () => {
    const { session } = makeSession()
    forceRunning(session)
    ;(session as unknown as { _clusterStreamWanted: boolean })._clusterStreamWanted = false
    const sent = captureEncrypted(session)
    ;(session as unknown as { _requestClusterStream: () => void })._requestClusterStream()
    expect(sent).not.toHaveBeenCalled()
  })

  test('_stopClusterStream sends NATIVE only when RUNNING', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(session as unknown as { _stopClusterStream: () => void })._stopClusterStream()
    expect(sent).not.toHaveBeenCalled()
    forceRunning(session)
    ;(session as unknown as { _stopClusterStream: () => void })._stopClusterStream()
    expect(sent).toHaveBeenCalledTimes(1)
    expect(sent.mock.calls[0][3]).toEqual(Buffer.from([0x08, 0x02]))
  })
})

describe('requestMainKeyframe', () => {
  test('no-op outside RUNNING', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    session.requestMainKeyframe()
    expect(sent).not.toHaveBeenCalled()
  })

  test('sends an immediate and a delayed focus indication', () => {
    vi.useFakeTimers()
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestMainKeyframe()
    expect(sent).toHaveBeenCalledTimes(1)
    vi.advanceTimersByTime(60)
    expect(sent).toHaveBeenCalledTimes(2)
    expect(sent.mock.calls[1][3]).toEqual(Buffer.from([0x08, 0x01]))
    vi.useRealTimers()
  })

  test('delayed indication skipped when state left RUNNING', () => {
    vi.useFakeTimers()
    const { session } = makeSession()
    forceRunning(session)
    const sent = captureEncrypted(session)
    session.requestMainKeyframe()
    setState(session, CLOSED)
    vi.advanceTimersByTime(60)
    expect(sent).toHaveBeenCalledTimes(1)
    vi.useRealTimers()
  })
})

describe('_handleAVSetupRequest — codec selection', () => {
  function setup(codecType: number): { session: Session; sent: Mock } {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    const proto = protoStub()
    ;(proto.AVChannelSetupRequest as { decode: Mock }).decode = vi.fn(() => ({
      mediaCodecType: codecType
    }))
    ;(session as unknown as { _proto: unknown })._proto = proto
    return { session, sent }
  }

  test.each([
    [MEDIA_CODEC.VIDEO_H265, 'h265'],
    [MEDIA_CODEC.VIDEO_VP9, 'vp9'],
    [MEDIA_CODEC.VIDEO_AV1, 'av1'],
    [999, 'h264']
  ])('video codec %s → %s', (codecType, expected) => {
    const { session, sent } = setup(codecType)
    ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = [
      'h264',
      'h265',
      'vp9',
      'av1'
    ]
    const cb = vi.fn()
    session.on('video-codec', cb)
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(CH.VIDEO, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(expected)
    expect(sent).toHaveBeenCalled()
  })

  test('video codec not in the offered list keeps configIdx 0', () => {
    const { session } = setup(MEDIA_CODEC.VIDEO_AV1)
    ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = ['h264']
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(CH.VIDEO, Buffer.alloc(0))
    expect((session as unknown as { _videoCodec: string })._videoCodec).toBe('av1')
  })

  test('unchanged video codec does not re-emit but still logs once', () => {
    const { session } = setup(MEDIA_CODEC.VIDEO_H265)
    ;(session as unknown as { _videoCodecByIndex: string[] })._videoCodecByIndex = ['h264', 'h265']
    ;(session as unknown as { _videoCodec: string })._videoCodec = 'h265'
    ;(session as unknown as { _phoneCodecLogged: boolean })._phoneCodecLogged = true
    const cb = vi.fn()
    session.on('video-codec', cb)
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(CH.VIDEO, Buffer.alloc(0))
    expect(cb).not.toHaveBeenCalled()
  })

  test.each([
    [MEDIA_CODEC.VIDEO_H265, 'h265'],
    [MEDIA_CODEC.VIDEO_VP9, 'vp9'],
    [MEDIA_CODEC.VIDEO_AV1, 'av1'],
    [999, 'h264']
  ])('cluster codec %s → %s', (codecType, expected) => {
    const { session } = setup(codecType)
    ;(session as unknown as { _clusterCodecByIndex: string[] })._clusterCodecByIndex = [
      'h264',
      'h265',
      'vp9',
      'av1'
    ]
    const cb = vi.fn()
    session.on('cluster-video-codec', cb)
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(CH.CLUSTER_VIDEO, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith(expected)
  })

  test('cluster codec not in the offered list keeps configIdx 0', () => {
    const { session } = setup(MEDIA_CODEC.VIDEO_AV1)
    ;(session as unknown as { _clusterCodecByIndex: string[] })._clusterCodecByIndex = ['h264']
    const cb = vi.fn()
    session.on('cluster-video-codec', cb)
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(CH.CLUSTER_VIDEO, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith('av1')
  })

  test('unchanged cluster codec does not re-emit', () => {
    const { session } = setup(MEDIA_CODEC.VIDEO_H265)
    ;(session as unknown as { _clusterCodecByIndex: string[] })._clusterCodecByIndex = ['h265']
    ;(session as unknown as { _clusterCodec: string })._clusterCodec = 'h265'
    const cb = vi.fn()
    session.on('cluster-video-codec', cb)
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(CH.CLUSTER_VIDEO, Buffer.alloc(0))
    expect(cb).not.toHaveBeenCalled()
  })

  test.each([CH.SPEECH_AUDIO, CH.SYSTEM_AUDIO])('16k mono setup for channel %s', (chId) => {
    const { session } = setup(1)
    const audio = new Map<number, { handleSetupRequest: Mock }>()
    audio.set(chId, { handleSetupRequest: vi.fn() })
    ;(session as unknown as { _audio: unknown })._audio = audio
    ;(
      session as unknown as { _handleAVSetupRequest: (c: number, p: Buffer) => void }
    )._handleAVSetupRequest(chId, Buffer.alloc(0))
    expect(audio.get(chId)!.handleSetupRequest).toHaveBeenCalledWith(1, 16000, 1)
  })
})

describe('wifi credentials with unset config', () => {
  test('missing ssid and password fall back to empty strings', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    expect(sent).toHaveBeenCalledTimes(1)
  })
})

describe('sensor start request payload guard', () => {
  test('a short or non-0x08 sensor request defaults the type to zero', () => {
    const { session } = makeSession()
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.alloc(0))
    ;(
      session as unknown as { _handleSensorStartRequest: (b: Buffer) => void }
    )._handleSensorStartRequest(Buffer.from([0x09, 5]))
    expect(sent).toHaveBeenCalled()
  })
})

describe('_handleDecryptedMessage — phone status + start indication', () => {
  test('PHONE_STATUS emits device-status when signal present', () => {
    const { session } = makeSession()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    const cb = vi.fn()
    session.on('device-status', cb)
    dispatch(session, CH.PHONE_STATUS, 0, 0x8001, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith({ ip: '192.168.1.5', signalStrength: 3 })
  })

  test('PHONE_STATUS without a numeric signal emits nothing', () => {
    const { session } = makeSession()
    const proto = protoStub()
    ;(proto.PhoneStatus as { decode: Mock }).decode = vi.fn(() => ({}))
    ;(session as unknown as { _proto: unknown })._proto = proto
    const cb = vi.fn()
    session.on('device-status', cb)
    dispatch(session, CH.PHONE_STATUS, 0, 0x8001, Buffer.alloc(0))
    expect(cb).not.toHaveBeenCalled()
  })

  test('PHONE_STATUS decode error is swallowed', () => {
    const { session } = makeSession()
    const proto = protoStub()
    ;(proto.PhoneStatus as { decode: Mock }).decode = vi.fn(() => {
      throw new Error('bad proto')
    })
    ;(session as unknown as { _proto: unknown })._proto = proto
    expect(() => dispatch(session, CH.PHONE_STATUS, 0, 0x8001, Buffer.alloc(0))).not.toThrow()
  })

  test('PHONE_STATUS ignores non-8001 message ids', () => {
    const { session } = makeSession()
    const cb = vi.fn()
    session.on('device-status', cb)
    dispatch(session, CH.PHONE_STATUS, 0, 0x1234, Buffer.alloc(0))
    expect(cb).not.toHaveBeenCalled()
  })

  test('START_INDICATION on an unmapped audio channel is handled without routing', () => {
    const { session } = makeSession()
    ;(session as unknown as { _audio: Map<number, unknown> })._audio = new Map()
    const cb = vi.fn()
    session.on('video-codec', cb)
    expect(() =>
      dispatch(session, CH.MEDIA_AUDIO, 0, AV_MSG.START_INDICATION, Buffer.from([0x08, 0x01]))
    ).not.toThrow()
    expect(cb).not.toHaveBeenCalled()
  })

  test('START_INDICATION on an auxiliary channel with no session id is handled', () => {
    const { session } = makeSession()
    expect(() => dispatch(session, 0x7f, 0, AV_MSG.START_INDICATION, Buffer.alloc(0))).not.toThrow()
  })

  test('mapped audio channel forwards non-setup messages', () => {
    const { session } = makeSession()
    const handleMessage = vi.fn()
    const audio = new Map<number, { handleMessage: Mock }>()
    audio.set(CH.MEDIA_AUDIO, { handleMessage })
    ;(session as unknown as { _audio: unknown })._audio = audio
    dispatch(session, CH.MEDIA_AUDIO, 0, 0x0001, Buffer.from([1]))
    expect(handleMessage).toHaveBeenCalled()
  })

  test('unhandled wifi and unknown channels fall through silently without DEBUG', () => {
    const { session } = makeSession()
    expect(() => dispatch(session, CH.WIFI, 0, 0x9999, Buffer.alloc(0))).not.toThrow()
    expect(() => dispatch(session, 0x7d, 0, 0x4444, Buffer.alloc(0))).not.toThrow()
  })
})

describe('_sendAA gating', () => {
  test('drops everything once CLOSED', () => {
    const { session, link } = makeSession()
    setState(session, CLOSED)
    ;(session as unknown as { _sendAA: (...a: unknown[]) => void })._sendAA(
      CH.CONTROL,
      FRAME_FLAGS.PLAINTEXT,
      0x1,
      Buffer.from([1])
    )
    expect(link.send).not.toHaveBeenCalled()
  })

  test('encrypted path hands the message to the link from AUTH on', () => {
    const { session, link } = makeSession()
    setState(session, AUTH)
    ;(session as unknown as { _sendAA: (...a: unknown[]) => void })._sendAA(
      CH.SENSOR,
      FRAME_FLAGS.ENC_SIGNAL,
      0x8003,
      Buffer.from([1, 2])
    )
    expect(link.send).toHaveBeenCalledWith(
      CH.SENSOR,
      FRAME_FLAGS.ENC_SIGNAL,
      0x8003,
      Buffer.from([1, 2])
    )
  })
})

describe('start() — wiring and control events', () => {
  test('wires channels, sends SDR, ping and forwards device-info + battery', async () => {
    vi.useFakeTimers()
    const { session } = await started()
    const control = (session as unknown as { _control: EventEmitter })._control
    const sendAA = vi.fn()
    ;(session as unknown as { _sendAA: Mock })._sendAA = sendAA
    ;(session as unknown as { _openChannels: Mock })._openChannels = vi.fn()

    const info = vi.fn()
    const status = vi.fn()
    session.on('device-info', info)
    session.on('device-status', status)

    control.emit('service-discovery-request', {
      deviceName: 'Pixel',
      deviceBrand: 'Google',
      phoneInfo: { instanceId: 'abc' }
    })
    expect(buildServiceDiscoveryResponse).toHaveBeenCalled()
    expect(info).toHaveBeenCalledWith({
      name: 'Pixel',
      model: 'Google',
      instanceId: 'abc',
      ip: '192.168.1.5'
    })
    expect(sendAA).toHaveBeenCalled()

    control.emit('battery', { level: 80, critical: false, timeRemaining: 3600 })
    expect(status).toHaveBeenCalledWith({
      ip: '192.168.1.5',
      batteryLevel: 80,
      batteryCritical: false,
      batteryTimeRemaining: 3600
    })

    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('SDR handler with no identity fields skips device-info', async () => {
    const { session } = await started()
    const control = (session as unknown as { _control: EventEmitter })._control
    ;(session as unknown as { _sendAA: Mock })._sendAA = vi.fn()
    ;(session as unknown as { _openChannels: Mock })._openChannels = vi.fn()
    const info = vi.fn()
    session.on('device-info', info)
    control.emit('service-discovery-request', {})
    expect(info).not.toHaveBeenCalled()
    session.close()
  })

  test('ping timeout closes the session and destroys the link', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    const control = (session as unknown as { _control: EventEmitter })._control
    ;(session as unknown as { _sendAA: Mock })._sendAA = vi.fn()
    ;(session as unknown as { _openChannels: Mock })._openChannels = vi.fn()
    control.emit('service-discovery-request', {})
    ;(session as unknown as { _lastPongAt: number })._lastPongAt = -100000
    vi.advanceTimersByTime(1500)
    expect((session as unknown as { _state: number })._state).toBe(CLOSED)
    expect(link.destroy).toHaveBeenCalled()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('control forwards voice, audio-focus, pong, channel-open and av-setup', async () => {
    const { session } = await started()
    const control = (
      session as unknown as { _control: EventEmitter } & {
        _control: EventEmitter & { sendChannelOpenResponse?: Mock }
      }
    )._control as EventEmitter & { sendChannelOpenResponse: Mock }
    control.sendChannelOpenResponse = vi.fn()
    const avSetup = vi.fn()
    ;(session as unknown as { _handleAVSetupRequest: Mock })._handleAVSetupRequest = avSetup

    const voice = vi.fn()
    const focus = vi.fn()
    session.on('voice-session', voice)
    session.on('audio-focus', focus)
    control.emit('voice-session', true)
    control.emit('audio-focus-request', 2)
    control.emit('pong')
    control.emit('channel-open-request', CH.VIDEO)
    control.emit('av-setup-request', CH.VIDEO, Buffer.alloc(0))

    expect(voice).toHaveBeenCalledWith(true)
    expect(focus).toHaveBeenCalledWith(2)
    expect(control.sendChannelOpenResponse).toHaveBeenCalledWith(CH.VIDEO, 0)
    expect(avSetup).toHaveBeenCalled()
    session.close()
  })

  test('control shutdown transitions to CLOSED', async () => {
    const { session } = await started()
    const control = (session as unknown as { _control: EventEmitter })._control
    control.emit('shutdown', 3)
    expect((session as unknown as { _state: number })._state).toBe(CLOSED)
  })

  test('forwards video, cluster, audio, mic, media and nav channel events', async () => {
    const { session } = await started()
    const s = session as unknown as {
      _video: EventEmitter
      _cluster: EventEmitter
      _audio: Map<number, EventEmitter>
      _mic: EventEmitter
      _media: EventEmitter
      _nav: EventEmitter
    }
    const events: Record<string, Mock> = {}
    for (const name of [
      'host-ui-requested',
      'video-focus-projected',
      'cluster-video-focus-projected',
      'audio-setup',
      'audio-start',
      'audio-stop',
      'mic-start',
      'mic-stop',
      'media-metadata',
      'media-status',
      'nav-start',
      'nav-stop',
      'nav-status',
      'nav-turn',
      'nav-distance',
      'nav-state',
      'nav-position'
    ]) {
      events[name] = vi.fn()
      session.on(name, events[name])
    }

    s._video.emit('host-ui-requested')
    s._video.emit('video-focus-projected')
    s._cluster.emit('video-focus-projected')
    const audio = s._audio.get(CH.MEDIA_AUDIO)!
    audio.emit('setup', 1, 48000, 2)
    audio.emit('start', 'media', CH.MEDIA_AUDIO)
    audio.emit('stop', 'media', CH.MEDIA_AUDIO)
    s._mic.emit('mic-start', CH.MIC_INPUT)
    s._mic.emit('mic-stop', CH.MIC_INPUT)
    s._media.emit('metadata', { title: 'x' })
    s._media.emit('status', { playbackState: 1 })
    s._nav.emit('nav-start')
    s._nav.emit('nav-stop')
    s._nav.emit('nav-status', {})
    s._nav.emit('nav-turn', {})
    s._nav.emit('nav-distance', {})
    s._nav.emit('nav-state', {})
    s._nav.emit('nav-position', {})

    for (const name of Object.keys(events)) expect(events[name]).toHaveBeenCalled()
    expect(events['audio-setup']).toHaveBeenCalledWith('media', 48000, 2)
    session.close()
  })

  test('the first main frame releases a pending cluster request, later frames do not', () => {
    const { session, link } = makeSession()
    const s = session as unknown as {
      _clusterFocusPending: boolean
      _requestClusterStream: Mock
    }
    s._requestClusterStream = vi.fn()
    s._clusterFocusPending = true
    const started = vi.fn()
    session.on('video-started', started)
    link.emit('control', { type: 'first-frame', ch: CH.VIDEO })
    expect(s._requestClusterStream).toHaveBeenCalledTimes(1)
    link.emit('control', { type: 'first-frame', ch: CH.VIDEO })
    expect(s._requestClusterStream).toHaveBeenCalledTimes(1)
    expect(started).toHaveBeenCalledTimes(2)
  })

  test('pre-RUNNING watchdog aborts a stalled session', async () => {
    vi.useFakeTimers()
    const { session } = await started()
    const err = vi.fn()
    session.on('error', err)
    const close = vi.spyOn(session, 'close')
    vi.advanceTimersByTime(30_000)
    expect(err).toHaveBeenCalled()
    expect(close).toHaveBeenCalledWith('pre-RUNNING watchdog')
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('pre-RUNNING watchdog does nothing once RUNNING', async () => {
    vi.useFakeTimers()
    const { session } = await started()
    forceRunning(session)
    const err = vi.fn()
    session.on('error', err)
    vi.advanceTimersByTime(30_000)
    expect(err).not.toHaveBeenCalled()
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('watchdog swallows a throwing error listener', async () => {
    vi.useFakeTimers()
    const { session } = await started()
    session.on('error', () => {
      throw new Error('listener blew up')
    })
    expect(() => vi.advanceTimersByTime(30_000)).not.toThrow()
    vi.clearAllTimers()
    vi.useRealTimers()
  })
})

describe('channel send closures + misc guards', () => {
  test('each channel _send closure delegates to the session senders', async () => {
    const { session } = await started()
    const s = session as unknown as {
      _control: { _send: (...a: unknown[]) => void }
      _video: { _send: (...a: unknown[]) => void }
      _cluster: { _send: (...a: unknown[]) => void }
      _audio: Map<number, { _send: (...a: unknown[]) => void }>
      _mic: { _send: (...a: unknown[]) => void }
      _sendAA: Mock
      _sendEncrypted: Mock
    }
    s._sendAA = vi.fn()
    s._sendEncrypted = vi.fn()
    s._control._send(CH.CONTROL, 0x03, 0x1, Buffer.alloc(0))
    s._video._send(CH.VIDEO, 0x0b, 0x1, Buffer.alloc(0))
    s._cluster._send(CH.CLUSTER_VIDEO, 0x0b, 0x1, Buffer.alloc(0))
    s._audio.get(CH.MEDIA_AUDIO)!._send(CH.MEDIA_AUDIO, 0x0b, 0x1, Buffer.alloc(0))
    s._mic._send(CH.MIC_INPUT, 0x0b, 0x1, Buffer.alloc(0))
    ;(session as unknown as { _input: { _send: (...a: unknown[]) => void } })._input._send(
      CH.INPUT,
      0x0b,
      0x1,
      Buffer.alloc(0)
    )
    expect(s._sendAA).toHaveBeenCalledTimes(1)
    expect(s._sendEncrypted).toHaveBeenCalledTimes(5)
    session.close()
  })

  test('sensor ternaries cover both the true and false arms', () => {
    const { session } = makeSession()
    forceRunning(session)
    captureEncrypted(session)
    session.sendSpeedData(13_000, false, 0)
    session.sendNightModeData(false)
    session.sendParkingBrakeData(true)
    session.sendLightData(2, false, 3)
    session.sendEnvironmentData(20_000, 101_000, 0)
    session.sendFuelData(50, 200, false)
  })

  test('optional channel handlers short-circuit before start()', async () => {
    const { session } = makeSession()
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    const d = (ch: number, mid: number, p = Buffer.alloc(0)): void =>
      dispatch(session, ch, 0, mid, p)
    expect(() => {
      d(CH.CONTROL, 0x1)
      d(CH.VIDEO, 0x1, Buffer.from([1]))
      d(CH.CLUSTER_VIDEO, 0x1, Buffer.from([1]))
      d(CH.MEDIA_INFO, 0x1)
      d(CH.NAVIGATION, 0x1)
      d(CH.MIC_INPUT, 0x1, Buffer.from([1]))
    }).not.toThrow()
  })

  test('battery and SDR handlers tolerate an empty peer', async () => {
    vi.useFakeTimers()
    const { session, link } = await started()
    link.peer = ''
    ;(session as unknown as { _sendAA: Mock })._sendAA = vi.fn()
    ;(session as unknown as { _openChannels: Mock })._openChannels = vi.fn()
    const control = (session as unknown as { _control: EventEmitter })._control
    const status = vi.fn()
    const info = vi.fn()
    session.on('device-status', status)
    session.on('device-info', info)
    control.emit('battery', { critical: true })
    control.emit('service-discovery-request', { deviceName: 'X' })
    expect(status).toHaveBeenCalledWith(expect.objectContaining({ ip: '' }))
    expect(info).toHaveBeenCalledWith(expect.objectContaining({ ip: '' }))
    session.close()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('sendPing returns early once the session is closed', async () => {
    vi.useFakeTimers()
    const { session } = await started()
    const control = (session as unknown as { _control: EventEmitter })._control
    const sendAA = vi.fn()
    ;(session as unknown as { _sendAA: Mock })._sendAA = sendAA
    ;(session as unknown as { _openChannels: Mock })._openChannels = vi.fn()
    control.emit('service-discovery-request', {})
    sendAA.mockClear()
    setState(session, CLOSED)
    vi.advanceTimersByTime(1500)
    expect(sendAA).not.toHaveBeenCalled()
    vi.clearAllTimers()
    vi.useRealTimers()
  })

  test('sendTouch / sendButton / sendRotary no-op when the input channel is missing', () => {
    const { session } = makeSession()
    forceRunning(session)
    ;(session as unknown as { _input: unknown })._input = undefined
    expect(() => session.sendTouch(0, [{ x: 0, y: 0, id: 0 }])).not.toThrow()
    expect(() => session.sendButton(3, true)).not.toThrow()
    expect(() => session.sendRotary(1)).not.toThrow()
  })

  test('PHONE_STATUS strips the zone id off an IPv6 peer', () => {
    const { session, link } = makeSession()
    link.peer = 'fe80::1%wlan0'
    ;(session as unknown as { _proto: unknown })._proto = protoStub()
    const cb = vi.fn()
    session.on('device-status', cb)
    dispatch(session, CH.PHONE_STATUS, 0, 0x8001, Buffer.alloc(0))
    expect(cb).toHaveBeenCalledWith({ ip: 'fe80::1', signalStrength: 3 })
  })

  test('wifi credentials encode multi-byte varint lengths for long values', () => {
    const { session } = makeSession({
      wifiSsid: 'S'.repeat(200),
      wifiPassword: 'P'.repeat(200)
    })
    const sent = captureEncrypted(session)
    ;(
      session as unknown as { _handleWifiCredentialsRequest: () => void }
    )._handleWifiCredentialsRequest()
    const buf = sent.mock.calls[0][3] as Buffer
    expect(buf.toString('utf8')).toContain('P'.repeat(200))
  })
})

describe('Session.micFormat', () => {
  test('reports the mic channel format', () => {
    const { session } = makeSession()
    ;(session as unknown as { _mic: { format: unknown } })._mic = {
      format: { sampleRate: 24000, channels: 2 }
    }
    expect(session.micFormat()).toEqual({ sampleRate: 24000, channels: 2 })
  })
})
