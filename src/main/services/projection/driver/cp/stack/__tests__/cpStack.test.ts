import { EventEmitter } from 'node:events'
import {
  closeMicUplink,
  closeScreenReceiver,
  onAudioReceiverStarted,
  openAudioReceiver,
  openMicUplink,
  setAudioReceiverActive,
  setAudioReceiverVolume
} from '@main/services/video/GstVideo'

type CpStackControl = {
  setStreamVolume(audioType: number, level: number, rampMs: number): void
  setAudioActive(active: boolean): void
}

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { encodeBplist } from '../bplist'
import { ControlCipher } from '../controlCipher'
import { buildResponse, parseMessages, type RtspRequest } from '../rtspMessage'
import type { CpStackConfig } from '../types'

const reg = vi.hoisted(() => {
  const audioStreams: Record<string, unknown>[] = []
  const screens: Record<string, unknown>[] = []
  const decoders: Record<string, unknown>[] = []
  const uplinks: Record<string, unknown>[] = []
  const tunnels: Record<string, unknown>[] = []
  const timings: Record<string, unknown>[] = []
  const keepAlives: Record<string, unknown>[] = []
  const eventServers: Record<string, unknown>[] = []
  const relays: Record<string, unknown>[] = []
  return {
    audioStreams,
    screens,
    decoders,
    uplinks,
    tunnels,
    timings,
    keepAlives,
    eventServers,
    relays,
    createServer: vi.fn(),
    createConnection: vi.fn(),
    gst: {
      openVideoReceiver: vi.fn(),
      closeVideoReceiver: vi.fn(),
      setActiveFeeder: vi.fn(),
      onAudioStarted: vi.fn(),
      openAudio: vi.fn(async () => ({ streamId: 1, dataPort: 6000, controlPort: 6001 })),
      setAudioVolume: vi.fn(),
      closeAudio: vi.fn(),
      openMic: vi.fn(() => 2),
      closeMic: vi.fn(),
      setAudioActive: vi.fn()
    }
  }
})

vi.mock('node:net', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:net')>()
  const patched = {
    ...actual,
    createServer: reg.createServer,
    createConnection: reg.createConnection
  }
  return { ...patched, default: patched }
})

vi.mock('@main/services/video/gstHost', () => ({
  gstHost: reg.gst,
  VIDEO_PLANE_MAIN: 0x7a000001,
  VIDEO_PLANE_CLUSTER_RECV: 0x7a000010
}))

// In-process receiver mock: 0 = not available (the ScreenStream fallback); a test sets a port
// to enable it.
const inProcPort = { value: 0 }
vi.mock('@main/services/video/GstVideo', () => ({
  openScreenReceiver: vi.fn(() => inProcPort.value),
  closeScreenReceiver: vi.fn(),
  // null means "no in-process receiver", so the host path stays the default in tests
  openAudioReceiver: vi.fn(() => null),
  onAudioReceiverStarted: vi.fn(),
  setAudioReceiverActive: vi.fn(),
  setAudioReceiverVolume: vi.fn(),
  closeAudioReceiver: vi.fn(),
  openMicUplink: vi.fn(() => null),
  closeMicUplink: vi.fn()
}))

vi.mock('../screenStream', async () => {
  const { EventEmitter } = await import('node:events')
  class ScreenStream extends EventEmitter {
    key: Buffer
    stop = vi.fn()
    listen = vi.fn(async () => 42000)
    constructor(key: Buffer) {
      super()
      this.key = key
      reg.screens.push(this as unknown as Record<string, unknown>)
    }
  }
  return { ScreenStream }
})

vi.mock('../rtpAudioDecoder', async () => {
  const { EventEmitter } = await import('node:events')
  class CpRtpAudioDecoder extends EventEmitter {
    opts: Record<string, unknown>
    start = vi.fn(async () => true)
    write = vi.fn()
    stop = vi.fn()
    constructor(opts: Record<string, unknown>) {
      super()
      this.opts = opts
      reg.decoders.push(this as unknown as Record<string, unknown>)
    }
  }
  return { CpRtpAudioDecoder }
})

vi.mock('../micUplink', () => {
  class CpMicUplink {
    opts: Record<string, unknown>
    start = vi.fn()
    stop = vi.fn()
    write = vi.fn()
    constructor(opts: Record<string, unknown>) {
      this.opts = opts
      reg.uplinks.push(this as unknown as Record<string, unknown>)
    }
  }
  return { CpMicUplink }
})

vi.mock('../iapTunnel', async () => {
  const { EventEmitter } = await import('node:events')
  class IapTunnel extends EventEmitter {
    shared: Buffer
    seed: unknown
    listen = vi.fn(async () => 43000)
    stop = vi.fn()
    constructor(shared: Buffer, seed: unknown) {
      super()
      this.shared = shared
      this.seed = seed
      reg.tunnels.push(this as unknown as Record<string, unknown>)
    }
  }
  return { IapTunnel }
})

vi.mock('../keepAliveServer', () => {
  class KeepAliveServer {
    listen = vi.fn(async () => 44000)
    stop = vi.fn()
    constructor() {
      reg.keepAlives.push(this as unknown as Record<string, unknown>)
    }
  }
  return { KeepAliveServer }
})

vi.mock('../timingServer', () => {
  class TimingSync {
    start = vi.fn()
    stop = vi.fn()
    syncedNtp = vi.fn(() => 999n)
    listen = vi.fn(async () => 45000)
    constructor() {
      reg.timings.push(this as unknown as Record<string, unknown>)
    }
  }
  return { TimingSync }
})

type FakeSock = EventEmitter & {
  write: ReturnType<typeof vi.fn>
  destroy: ReturnType<typeof vi.fn>
  remoteAddress: string
  remotePort: number
}

function fakeSock(remote = 'fe80::1'): FakeSock {
  const s = new EventEmitter() as FakeSock
  s.write = vi.fn()
  s.destroy = vi.fn()
  s.remoteAddress = remote
  s.remotePort = 5000
  return s
}

function fakeServer(port = 46000): EventEmitter & Record<string, unknown> {
  const s = new EventEmitter() as EventEmitter & Record<string, unknown>
  s.listen = vi.fn((_opts: unknown, cb?: () => void) => {
    cb?.()
    return s
  })
  s.close = vi.fn()
  s.address = vi.fn(() => ({ port }))
  return s
}

function baseCfg(over: Partial<CpStackConfig> = {}): CpStackConfig {
  return {
    deviceName: 'LIVI',
    oemLabel: 'LIVI',
    icons: [],
    deviceId: 'AA:BB:CC:DD:EE:FF',
    btMac: 'AA:BB:CC:DD:EE:FF',
    sourceVersion: '1.0',
    hevc: true,
    h264: false,
    main: { widthPixels: 1920, heightPixels: 1080, fps: 60 },
    cluster: { widthPixels: 1280, heightPixels: 720, fps: 60 },
    port: 7000,
    entertainmentSampleRate: 48000,
    disableAudioOutput: false,
    mfi: {
      certificate: vi.fn(async () => Buffer.from('cert')),
      sign: vi.fn(async () => Buffer.alloc(64, 7)),
      protocolMajor: vi.fn(async () => 3)
    },
    ...over
  } as CpStackConfig
}

type Internals = {
  _conns: Set<FakeSock>
  _sessionSock: Map<Session, FakeSock>
  _active: Session | null
  _liveSession: Session | null
  _closing: boolean
  _nightMode: boolean | null
  _clusterWantActive: boolean
  _videoActive: boolean
  _activeUplinks: Set<unknown>
  _handle(req: RtspRequest, s: Session): Promise<unknown>
  _handleSetup(req: RtspRequest, s: Session): Promise<unknown>
  _handleTeardown(req: RtspRequest, s: Session): unknown
  _handleCommand(req: RtspRequest, s: Session): unknown
  _buildFeedback(s: Session): unknown
  _setupAudio(sd: Record<string, unknown>, s: Session, type: number): Promise<unknown>
  _setupScreen(sd: Record<string, unknown>, s: Session, isCluster?: boolean): Promise<number>
  _setupDataStream(sd: Record<string, unknown>, s: Session): Promise<unknown>
  _openEventChannel(s: Session): Promise<number>
  _openIapMessageRelay(s: Session): void
  _sendEventCommand(s: Session, body: Buffer): void
  _onEventMessage(s: Session, msg: RtspRequest): void
  _teardown(s: Session): void
  _activateClusterStream(s: Session): void
}

type Session = Record<string, unknown> & {
  pairVerify: Record<string, unknown>
  audioMeta: Record<string, unknown>[]
}

function internals(stack: unknown): Internals {
  return stack as Internals
}

function attach(stack: unknown, remote = 'fe80::1'): { session: Session; sock: FakeSock } {
  const sock = fakeSock(remote)
  ;(stack as { attachSocket(s: unknown): void }).attachSocket(sock)
  const session = [...internals(stack)._sessionSock.keys()].at(-1) as Session
  return { session, sock }
}

function stubVerify(session: Session, over: Record<string, unknown> = {}): void {
  session.pairVerify = {
    sharedSecret: Buffer.alloc(32, 9),
    controllerId: 'ctrl-1',
    controlKeys: null,
    handle: vi.fn(() => Buffer.from([1])),
    ...over
  }
}

function req(method: string, path: string, body: Buffer = Buffer.alloc(0)): RtspRequest {
  return { method, path, protocol: 'RTSP/1.0', headers: { cseq: '1' }, body }
}

async function loadStack(debug = false): Promise<new (cfg: CpStackConfig) => EventEmitter> {
  vi.resetModules()
  vi.doMock('@main/constants', () => ({ DEBUG: debug }))
  const mod = await import('../cpStack')
  return mod.CpStack as unknown as new (
    cfg: CpStackConfig
  ) => EventEmitter
}

let logSpy: ReturnType<typeof vi.spyOn>
let warnSpy: ReturnType<typeof vi.spyOn>
const originalPlatform = process.platform

beforeEach(() => {
  reg.audioStreams.length = 0
  reg.screens.length = 0
  reg.decoders.length = 0
  reg.uplinks.length = 0
  reg.tunnels.length = 0
  reg.timings.length = 0
  reg.keepAlives.length = 0
  reg.eventServers.length = 0
  reg.relays.length = 0
  reg.createServer.mockReset()
  reg.createConnection.mockReset()
  reg.gst.openVideoReceiver.mockReset().mockResolvedValue({ port: 40000, receiverId: 7 })
  reg.gst.closeVideoReceiver.mockReset()
  reg.gst.setActiveFeeder.mockReset()
  reg.gst.onAudioStarted.mockReset()
  reg.gst.openAudio
    .mockReset()
    .mockResolvedValue({ streamId: 1, dataPort: 6000, controlPort: 6001 })
  reg.gst.setAudioVolume.mockReset()
  reg.gst.setAudioActive.mockReset()
  reg.gst.closeAudio.mockReset()
  reg.gst.openMic.mockReset().mockReturnValue(2)
  reg.gst.closeMic.mockReset()
  Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
  logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
  warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
})

afterEach(() => {
  Object.defineProperty(process, 'platform', { value: originalPlatform, configurable: true })
  vi.restoreAllMocks()
})

describe('CpStack lifecycle', () => {
  it('adopts a control socket and tears it down on stop', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { sock } = attach(stack)
    expect(internals(stack)._conns.size).toBe(1)
    ;(stack as unknown as { stop(): void }).stop()
    expect(sock.destroy).toHaveBeenCalled()
    expect(internals(stack)._conns.size).toBe(0)
  })

  it('logs socket errors and ends the live session when its control socket closes', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const ended = vi.fn()
    stack.on('session-ended', ended)
    const { session, sock } = attach(stack)
    internals(stack)._liveSession = session
    sock.emit('error', new Error('reset'))
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('socket error'))
    sock.emit('close')
    expect(ended).toHaveBeenCalledTimes(1)
  })

  it('does not emit session-ended when closing', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const ended = vi.fn()
    stack.on('session-ended', ended)
    const { session, sock } = attach(stack)
    internals(stack)._liveSession = session
    ;(stack as unknown as { stop(): void }).stop()
    sock.emit('close')
    expect(ended).not.toHaveBeenCalled()
  })

  it('exposes the active controller id', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    expect(
      (stack as unknown as { activeControllerId: string | null }).activeControllerId
    ).toBeNull()
    const { session } = attach(stack)
    stubVerify(session)
    internals(stack)._liveSession = session
    expect((stack as unknown as { activeControllerId: string | null }).activeControllerId).toBe(
      'ctrl-1'
    )
  })
})

describe('CpStack control data path', () => {
  async function flush(): Promise<void> {
    for (let i = 0; i < 6; i++) await Promise.resolve()
  }

  it('answers plaintext requests, then switches to encrypted framing', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session, sock } = attach(stack)
    const readKey = Buffer.alloc(32, 1)
    const writeKey = Buffer.alloc(32, 2)
    stubVerify(session, { controlKeys: { readKey, writeKey } })

    sock.emit('data', Buffer.from('OPTIONS * RTSP/1.0\r\nCSeq: 1\r\n\r\n'))
    await flush()
    expect(sock.write).toHaveBeenCalled()
    expect(logSpy).toHaveBeenCalledWith('[cpStack] control channel encrypted')

    const phone = new ControlCipher(writeKey, readKey)
    const enc = phone.encrypt(Buffer.from('OPTIONS * RTSP/1.0\r\nCSeq: 2\r\n\r\n'))
    sock.write.mockClear()
    sock.emit('data', enc)
    await flush()
    expect(sock.write).toHaveBeenCalled()
  })

  it('answers 500 when a handler throws', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session, sock } = attach(stack)
    session.pairVerify = {
      sharedSecret: null,
      handle: vi.fn(),
      controlKeys: null,
      controllerId: null
    }
    const body = encodeBplist({ streams: [{ type: 100 }] })
    const head = `SETUP rtsp://x RTSP/1.0\r\nCSeq: 3\r\nContent-Length: ${body.length}\r\n\r\n`
    sock.emit('data', Buffer.concat([Buffer.from(head), body]))
    await flush()
    const written = sock.write.mock.calls.map((c) => (c[0] as Buffer).toString('ascii')).join('')
    expect(written).toContain('500')
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('handler error'),
      expect.anything()
    )
  })
})

describe('CpStack _handle routing', () => {
  async function fresh(debug = false): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack(debug)
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('suppresses chatty OPTIONS and feedback from the log', async () => {
    const { stack, session } = await fresh()
    logSpy.mockClear()
    await internals(stack)._handle(req('OPTIONS', '*'), session)
    await internals(stack)._handle(req('POST', 'rtsp://x/feedback'), session)
    expect(logSpy).not.toHaveBeenCalledWith(expect.stringContaining('< OPTIONS'))
  })

  it('logs a non-chatty request and decodes its body under debug', async () => {
    const { stack, session } = await fresh(true)
    const body = encodeBplist({ hello: 'world' })
    await internals(stack)._handle(req('GET', 'rtsp://x/other', body), session)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('< GET'))
    expect(logSpy).toHaveBeenCalledWith('[cpStack]   body:', expect.any(String))
  })

  it('reports a non-plist body under debug', async () => {
    const { stack, session } = await fresh(true)
    await internals(stack)._handle(req('GET', 'rtsp://x/other', Buffer.from('nope')), session)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('not a plist'))
  })

  it('handles RECORD by promoting the live session and pushing night mode', async () => {
    const { stack, session } = await fresh()
    internals(stack)._nightMode = true
    session.eventSock = fakeSock()
    session.eventCipher = new ControlCipher(Buffer.alloc(32, 3), Buffer.alloc(32, 4))
    reg.createConnection.mockReturnValue(fakeSock())
    const active = vi.fn()
    stack.on('session-active', active)
    const res = await internals(stack)._handle(req('RECORD', 'rtsp://x'), session)
    expect(res).toMatchObject({ status: 200 })
    expect(active).toHaveBeenCalled()
    expect(internals(stack)._liveSession).toBe(session)
  })

  it('routes pair-setup and pair-verify to their handlers', async () => {
    const { stack, session } = await fresh()
    const setupHandle = vi.fn(() => Buffer.from([1]))
    session.pairSetup = { handle: setupHandle } as unknown as Record<string, unknown>
    stubVerify(session, { handle: vi.fn(() => Buffer.from([2])) })
    const r1 = (await internals(stack)._handle(
      req('POST', 'rtsp://x/pair-setup'),
      session
    )) as Record<string, unknown>
    const r2 = (await internals(stack)._handle(
      req('POST', 'rtsp://x/pair-verify'),
      session
    )) as Record<string, unknown>
    expect(setupHandle).toHaveBeenCalled()
    expect((r1.headers as Record<string, string>)['Content-Type']).toContain('pairing')
    expect((r2.headers as Record<string, string>)['Content-Type']).toContain('pairing')
  })

  it('answers auth-setup, and 400 for a malformed request', async () => {
    const { stack, session } = await fresh()
    const good = Buffer.concat([Buffer.from([1]), Buffer.alloc(32, 5)])
    const okRes = (await internals(stack)._handle(
      req('POST', 'rtsp://x/auth-setup', good),
      session
    )) as Record<string, unknown>
    expect((okRes.headers as Record<string, string>)['Content-Type']).toContain('octet-stream')
    const badRes = await internals(stack)._handle(
      req('POST', 'rtsp://x/auth-setup', Buffer.alloc(4)),
      session
    )
    expect(badRes).toMatchObject({ status: 400 })
  })

  it('answers /info, logging the phone dictionary and a non-plist body', async () => {
    const { stack, session } = await fresh(true)
    const ask = encodeBplist({ name: 'phone' })
    const res = (await internals(stack)._handle(
      req('GET', 'rtsp://x/info', ask),
      session
    )) as Record<string, unknown>
    expect((res.headers as Record<string, string>)['Content-Type']).toContain('plist')
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('/info request from phone'))
    await internals(stack)._handle(req('GET', 'rtsp://x/info', Buffer.from('x')), session)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('/info request body not a plist'))
  })

  it('routes command and feedback posts, and acks unrouted requests', async () => {
    const { stack, session } = await fresh()
    const cmd = await internals(stack)._handle(
      req('POST', 'rtsp://x/command', encodeBplist({ type: 'requestUI' })),
      session
    )
    expect(cmd).toMatchObject({ status: 200 })
    const fb = await internals(stack)._handle(req('POST', 'rtsp://x/feedback'), session)
    expect(fb).toMatchObject({ status: 200 })
    const other = await internals(stack)._handle(req('DELETE', 'rtsp://x/nowhere'), session)
    expect(other).toMatchObject({ status: 200 })
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('unhandled request'))
  })
})

describe('CpStack SETUP', () => {
  async function fresh(
    cfg?: Partial<CpStackConfig>
  ): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg(cfg))
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('rejects a SETUP body that is not a plist', async () => {
    const { stack, session } = await fresh()
    const res = await internals(stack)._handleSetup(
      req('SETUP', 'rtsp://x', Buffer.from('nope')),
      session
    )
    expect(res).toMatchObject({ status: 400 })
    expect(warnSpy).toHaveBeenCalledWith(
      expect.stringContaining('SETUP body is not a plist'),
      expect.any(String)
    )
  })

  it('answers a session-level SETUP with timing, event and keepAlive ports', async () => {
    const { stack, session } = await fresh()
    reg.createServer.mockImplementation((handler: unknown) => {
      const srv = fakeServer(46000)
      ;(srv as Record<string, unknown>)._handler = handler
      reg.eventServers.push(srv)
      return srv
    })
    const info = vi.fn()
    stack.on('device-info', info)
    const body = encodeBplist({
      name: 'iPhone',
      deviceID: 'AA:BB',
      macAddress: 'CC:DD',
      model: 'iPhone15',
      timingPort: 7010,
      keepAliveLowPower: true
    })
    const res = (await internals(stack)._handleSetup(
      req('SETUP', 'rtsp://x', body),
      session
    )) as Record<string, unknown>
    expect(info).toHaveBeenCalled()
    expect((res.headers as Record<string, string>)['Content-Type']).toContain('plist')
    expect(reg.timings[0]?.start).toHaveBeenCalled()
    expect(reg.keepAlives).toHaveLength(1)
  })

  it('omits identity, timing start and keepAlive when the phone provides none', async () => {
    const { stack, session } = await fresh({ hevc: false, cluster: undefined })
    reg.createServer.mockImplementation((handler: unknown) => {
      const srv = fakeServer()
      ;(srv as Record<string, unknown>)._handler = handler
      return srv
    })
    const info = vi.fn()
    stack.on('device-info', info)
    const body = encodeBplist({})
    await internals(stack)._handleSetup(req('SETUP', 'rtsp://x', body), session)
    expect(info).not.toHaveBeenCalled()
    expect(reg.timings[0]?.start).not.toHaveBeenCalled()
    expect(reg.keepAlives).toHaveLength(0)
  })

  it('sets up screen, audio, data and unknown streams', async () => {
    const { stack, session } = await fresh()
    const body = encodeBplist({
      streams: [
        { type: 110, streamConnectionID: 1 },
        { type: 111, streamConnectionID: 2 },
        { type: 100, streamConnectionID: 3, audioType: 'media', audioFormat: 0x800000 },
        { type: 130, clientTypeUUID: 'E9459FD0-BCAD-4C45-820F-1E72447EF2F2', seed: 5 },
        { type: 999 }
      ]
    })
    const res = (await internals(stack)._handleSetup(
      req('SETUP', 'rtsp://x', body),
      session
    )) as Record<string, unknown>
    const streams = res.body && Buffer.isBuffer(res.body) ? res.body : Buffer.alloc(0)
    expect(streams.length).toBeGreaterThan(0)
    expect(reg.screens.length).toBeGreaterThan(0)
    expect(reg.gst.openAudio).toHaveBeenCalledTimes(1)
    expect(reg.tunnels.length).toBe(1)
  })
})

describe('CpStack audio setup formats', () => {
  async function setupAudio(
    sd: Record<string, unknown>,
    type = 100
  ): Promise<{ stack: EventEmitter; session: Session; result: Record<string, unknown> }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const result = (await internals(stack)._setupAudio(sd, session, type)) as Record<
      string,
      unknown
    >
    return { stack, session, result }
  }

  it('throws when audio SETUP precedes pair-verify', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    session.pairVerify = { sharedSecret: null }
    await expect(
      internals(stack)._setupAudio({ audioFormat: 0x800000 }, session, 100)
    ).rejects.toThrow('before pair-verify')
  })

  it('opens an AAC-LC 48k stereo stream in the host', async () => {
    const { result } = await setupAudio({
      streamConnectionID: 1,
      audioType: 'media',
      audioFormat: 0x800000
    })
    expect(reg.gst.openAudio).toHaveBeenCalledTimes(1)
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({
      codec: 'aac-lc',
      clockRate: 48000,
      channels: 2
    })
    expect(result).toMatchObject({ type: 100, dataPort: 6000, controlPort: 6001 })
  })

  it('opens an AAC-LC 44.1k stream at its own clock rate', async () => {
    await setupAudio({
      streamConnectionID: 1,
      audioType: 'media',
      audioFormat: 0x400000,
      audioLatencyMs: 1000
    })
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({
      codec: 'aac-lc',
      clockRate: 44100,
      latencyMs: 1000
    })
  })

  it('opens an OPUS stream and its microphone once the phone starts sending', async () => {
    const { stack } = await setupAudio({
      streamConnectionID: 1,
      audioType: 'speechRecognition',
      audioFormat: 0x20000000,
      dataPort: 6000,
      framesPerPacket: 480
    })
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({ codec: 'opus', clockRate: 48000 })

    const active = vi.fn()
    stack.on('mic-active', active)
    reg.gst.onAudioStarted.mock.calls.at(-1)[0](1, 4242)

    expect(reg.gst.openMic).toHaveBeenCalledTimes(1)
    expect(reg.gst.openMic.mock.calls[0][1]).toMatchObject({ codec: 'opus', port: 6000 })
    expect(active).toHaveBeenCalledWith(true, expect.any(Number), 1)
  })

  it('opens the microphone in-process when the addon plays the stream', async () => {
    vi.mocked(openAudioReceiver).mockReturnValueOnce({
      streamId: 0x7c000001,
      dataPort: 5001,
      controlPort: 5002
    })
    vi.mocked(openMicUplink).mockReturnValueOnce(0x7d000001)
    const { stack, session } = await setupAudio({
      streamConnectionID: 1,
      audioType: 'speechRecognition',
      audioFormat: 0x20000000,
      dataPort: 6000,
      framesPerPacket: 480
    })
    expect(reg.gst.openAudio).not.toHaveBeenCalled()

    const active = vi.fn()
    stack.on('mic-active', active)
    vi.mocked(onAudioReceiverStarted).mock.calls.at(-1)?.[0](0x7c000001, 4242)

    expect(openMicUplink).toHaveBeenCalledTimes(1)
    expect(vi.mocked(openMicUplink).mock.calls[0][1]).toMatchObject({ codec: 'opus', port: 6000 })
    expect(reg.gst.openMic).not.toHaveBeenCalled()
    expect(active).toHaveBeenCalledWith(true, expect.any(Number), 1)

    internals(stack)._closeAudio(session.audioMeta[0])
    expect(closeMicUplink).toHaveBeenCalledWith(0x7d000001)
    expect(reg.gst.closeMic).not.toHaveBeenCalled()
  })

  it('warns and skips mic-active when the in-process mic uplink fails to open', async () => {
    vi.mocked(openAudioReceiver).mockReturnValueOnce({
      streamId: 0x7c000002,
      dataPort: 5003,
      controlPort: 5004
    })
    // openMicUplink defaults to returning null (no in-process uplink available).
    const { stack } = await setupAudio({
      streamConnectionID: 1,
      audioType: 'speechRecognition',
      audioFormat: 0x20000000,
      dataPort: 6000,
      framesPerPacket: 480
    })

    const active = vi.fn()
    stack.on('mic-active', active)
    vi.mocked(onAudioReceiverStarted).mock.calls.at(-1)?.[0](0x7c000002, 4242)

    expect(openMicUplink).toHaveBeenCalledTimes(1)
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('mic uplink failed to open'))
    expect(active).not.toHaveBeenCalled()
  })

  it('opens LPCM as samples on the wire', async () => {
    await setupAudio(
      { streamConnectionID: 1, audioType: 'telephony', audioFormat: 0x800, dataPort: 0 },
      100
    )
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({ codec: 'pcm' })
  })

  it('defaults an unknown audio format to 44.1k stereo', async () => {
    await setupAudio({ streamConnectionID: 1, audioType: 'alert', audioFormat: 0 }, 101)
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({
      codec: 'pcm',
      clockRate: 44100,
      channels: 2
    })
  })

  it('takes the short path to the sink for everything but media', async () => {
    await setupAudio({ streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 })
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({ realtime: false })

    reg.gst.openAudio.mockClear()
    await setupAudio({ streamConnectionID: 2, audioType: 'telephony', audioFormat: 0x800 }, 101)
    expect(reg.gst.openAudio.mock.calls[0][1]).toMatchObject({ realtime: true })
  })
})

describe('CpStack screen setup', () => {
  async function stackWith(
    cfg?: Partial<CpStackConfig>
  ): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg(cfg))
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('throws when a screen SETUP precedes pair-verify', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    session.pairVerify = { sharedSecret: null }
    await expect(internals(stack)._setupScreen({ streamConnectionID: 1 }, session)).rejects.toThrow(
      'before pair-verify'
    )
  })

  it('opens native receivers on linux for main and cluster screens', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const { stack, session } = await stackWith()
    ;(stack as unknown as { setVideoActive(a: boolean): void }).setVideoActive(true)
    internals(stack)._clusterWantActive = true
    const ready = vi.fn()
    stack.on('main-screen-ready', ready)
    const mainPort = await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    const clusterPort = await internals(stack)._setupScreen(
      { streamConnectionID: 2 },
      session,
      true
    )
    expect(mainPort).toBe(40000)
    expect(clusterPort).toBe(40000)
    expect(ready).toHaveBeenCalled()
    expect(reg.gst.setActiveFeeder).toHaveBeenCalled()
  })

  it('uses the in-process receiver off linux when the addon offers one', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    inProcPort.value = 40500
    try {
      const { stack, session } = await stackWith({ hevc: false })
      const codec = vi.fn()
      const ready = vi.fn()
      stack.on('video-codec', codec)
      stack.on('main-screen-ready', ready)

      const port = await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)

      expect(port).toBe(40500)
      expect(codec).toHaveBeenCalledWith('h264')
      expect(ready).toHaveBeenCalled()
      // the frames reach the plane inside the addon, so no ScreenStream is built here
      expect(session.screen).toBeFalsy()
    } finally {
      inProcPort.value = 0
    }
  })

  it('uses the in-process receiver off linux for the cluster screen', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    inProcPort.value = 40600
    try {
      const { stack, session } = await stackWith({ hevc: false })
      const clusterCodec = vi.fn()
      stack.on('cluster-video-codec', clusterCodec)

      const port = await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)

      expect(port).toBe(40600)
      expect(session.clusterScreenNativeId).toBeTruthy()
      expect(session.clusterScreenInProcess).toBe(true)
      expect(clusterCodec).toHaveBeenCalledWith('h264')
      expect(session.clusterScreen).toBeFalsy()
    } finally {
      inProcPort.value = 0
    }
  })

  it('reports h265 for the in-process path, activates a pending cluster once, then settles', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    inProcPort.value = 40700
    try {
      // default cfg.hevc is true; this exercises the h265 arm of the in-process codec pick
      const { stack, session } = await stackWith()
      internals(stack)._clusterWantActive = true
      const evSock = eventReady(session)
      const codec = vi.fn()
      const clusterCodec = vi.fn()
      stack.on('video-codec', codec)
      stack.on('cluster-video-codec', clusterCodec)

      await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
      expect(codec).toHaveBeenCalledWith('h265')
      // main screen just became ready, with a cluster pending: activated at once
      expect(evSock.write).toHaveBeenCalled()

      evSock.write.mockClear()
      await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
      // codec and readiness are already tracked: no repeat emit, no repeat activation
      expect(codec).toHaveBeenCalledTimes(1)
      expect(evSock.write).not.toHaveBeenCalled()

      await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)
      await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)
      expect(clusterCodec).toHaveBeenCalledTimes(1)
      expect(clusterCodec).toHaveBeenCalledWith('h265')
    } finally {
      inProcPort.value = 0
    }
  })

  it('reports the advertised codec once per native screen on linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const { stack, session } = await stackWith({ hevc: false })
    const codec = vi.fn()
    const clusterCodec = vi.fn()
    stack.on('video-codec', codec)
    stack.on('cluster-video-codec', clusterCodec)
    await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)
    await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)
    expect(codec).toHaveBeenCalledTimes(1)
    expect(codec).toHaveBeenCalledWith('h264')
    expect(clusterCodec).toHaveBeenCalledTimes(1)
    expect(clusterCodec).toHaveBeenCalledWith('h264')
  })

  it('builds a ScreenStream off linux and emits codec, config and frames', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const { stack, session } = await stackWith()
    internals(stack)._clusterWantActive = true
    const codec = vi.fn()
    const config = vi.fn()
    const frame = vi.fn()
    stack.on('video-codec', codec)
    stack.on('video-config', config)
    stack.on('video-frame', frame)
    await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    const screen = reg.screens[0]
    screen?.emit?.('codec', 'h264')
    screen?.emit?.('codec', 'h264')
    screen?.emit?.('config', Buffer.from('cfg'))
    screen?.emit?.('frame', Buffer.from('nal'))
    screen?.emit?.('frame', Buffer.from('nal2'))
    expect(codec).toHaveBeenCalledTimes(1)
    expect(config).toHaveBeenCalled()
    expect(frame).toHaveBeenCalledTimes(2)
  })

  it('emits cluster codec only once for the alt screen off linux', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const { stack, session } = await stackWith({ hevc: false })
    const codec = vi.fn()
    const frame = vi.fn()
    stack.on('cluster-video-codec', codec)
    stack.on('cluster-video-frame', frame)
    await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)
    const screen = reg.screens[0]
    screen?.emit?.('codec', 'h264')
    screen?.emit?.('codec', 'h264')
    screen?.emit?.('frame', Buffer.from('nal'))
    expect(codec).toHaveBeenCalledTimes(1)
    expect(frame).toHaveBeenCalledTimes(1)
  })
})

describe('CpStack data stream setup', () => {
  it('ignores a data stream with a foreign uuid', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const res = await internals(stack)._setupDataStream({ clientTypeUUID: 'other' }, session)
    expect(res).toBeNull()
  })

  it('throws when the iAP data stream precedes pair-verify', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    session.pairVerify = { sharedSecret: null }
    await expect(
      internals(stack)._setupDataStream(
        { clientTypeUUID: 'E9459FD0-BCAD-4C45-820F-1E72447EF2F2', seed: 1 },
        session
      )
    ).rejects.toThrow('before pair-verify')
  })

  it('opens the iAP tunnel and relays iap frames to the message relay', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const relay = fakeSock()
    session.iapRelay = relay
    const res = (await internals(stack)._setupDataStream(
      { clientTypeUUID: 'E9459FD0-BCAD-4C45-820F-1E72447EF2F2', seed: 3 },
      session
    )) as Record<string, unknown>
    expect(res).toMatchObject({ type: 130, dataPort: 43000 })
    reg.tunnels[0]?.emit?.('iap', Buffer.from('iapbytes'))
    expect(relay.write).toHaveBeenCalledWith(Buffer.from('iapbytes'))
  })
})

describe('CpStack event channel', () => {
  async function openChannel(withShared = true): Promise<{
    stack: EventEmitter
    session: Session
    server: EventEmitter & Record<string, unknown>
    connect: (s: FakeSock) => void
  }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session, withShared ? {} : { sharedSecret: null })
    let handler: ((s: unknown) => void) | undefined
    const server = fakeServer(46000)
    reg.createServer.mockImplementation((h: (s: unknown) => void) => {
      handler = h
      return server
    })
    const port = await internals(stack)._openEventChannel(session)
    expect(port).toBe(46000)
    return { stack, session, server, connect: (s: FakeSock) => handler?.(s) }
  }

  it('accepts the phone event connection, derives keys and answers its requests', async () => {
    const { session, connect } = await openChannel(true)
    const evSock = fakeSock()
    connect(evSock)
    const readKey = (session.eventCipher as ControlCipher)['readKey'] as Buffer
    const writeKey = (session.eventCipher as ControlCipher)['writeKey'] as Buffer
    const phone = new ControlCipher(writeKey, readKey)
    const request = phone.encrypt(Buffer.from('GET /x RTSP/1.0\r\nCSeq: 1\r\n\r\n'))
    evSock.emit('data', request)
    expect(evSock.write).toHaveBeenCalled()
  })

  it('warns when an event frame fails to decrypt', async () => {
    const { connect } = await openChannel(true)
    const evSock = fakeSock()
    connect(evSock)
    evSock.emit('data', Buffer.concat([Buffer.from([8, 0]), Buffer.alloc(30, 1)]))
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('event channel decrypt failed'))
  })

  it('ignores event data when no cipher was derived', async () => {
    const { connect } = await openChannel(false)
    const evSock = fakeSock()
    connect(evSock)
    expect(() => evSock.emit('data', Buffer.from([1, 2, 3]))).not.toThrow()
  })

  it('clears the active session when the event socket closes', async () => {
    const { stack, connect } = await openChannel(true)
    const evSock = fakeSock()
    connect(evSock)
    evSock.emit('error', new Error('x'))
    evSock.emit('close')
    expect(internals(stack)._active).toBeNull()
  })

  it('rejects when the event server errors', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const server = fakeServer()
    server.listen = vi.fn(() => {
      server.emit('error', new Error('EADDRINUSE'))
      return server
    })
    reg.createServer.mockReturnValue(server)
    await expect(internals(stack)._openEventChannel(session)).rejects.toThrow('EADDRINUSE')
  })

  it('resolves 0 when the event server reports no address', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const server = fakeServer()
    server.address = vi.fn(() => null)
    reg.createServer.mockReturnValue(server)
    await expect(internals(stack)._openEventChannel(session)).resolves.toBe(0)
  })
})

function eventReady(session: Session): FakeSock {
  const evSock = fakeSock()
  session.eventSock = evSock
  session.eventCipher = new ControlCipher(Buffer.alloc(32, 3), Buffer.alloc(32, 4))
  session.eventCseq = 0
  return evSock
}

describe('CpStack commands', () => {
  async function fresh(debug = false): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack(debug)
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  function cmd(type: string, params?: Record<string, unknown>): RtspRequest {
    return req('POST', 'rtsp://x/command', encodeBplist(params ? { type, params } : { type }))
  }

  it('emits host-ui-requested for requestUI', async () => {
    const { stack, session } = await fresh()
    const host = vi.fn()
    stack.on('host-ui-requested', host)
    internals(stack)._handleCommand(cmd('requestUI'), session)
    expect(host).toHaveBeenCalled()
  })

  it('emits disable-bluetooth with the device id', async () => {
    const { stack, session } = await fresh()
    const bt = vi.fn()
    stack.on('disable-bluetooth', bt)
    internals(stack)._handleCommand(cmd('disableBluetooth', { deviceID: 'AA:BB' }), session)
    expect(bt).toHaveBeenCalledWith('AA:BB')
  })

  it('tracks Siri speech mode transitions from modesChanged', async () => {
    const { stack, session } = await fresh(true)
    const speech = vi.fn()
    stack.on('speech-active', speech)
    internals(stack)._handleCommand(
      cmd('modesChanged', { appStates: [{ appStateID: 1, speechMode: 2 }] }),
      session
    )
    internals(stack)._handleCommand(
      cmd('modesChanged', { appStates: [{ appStateID: 1, speechMode: -1 }] }),
      session
    )
    expect(speech).toHaveBeenNthCalledWith(1, true)
    expect(speech).toHaveBeenNthCalledWith(2, false)
  })

  it('ignores modesChanged without a usable appStates list', async () => {
    const { stack, session } = await fresh()
    const speech = vi.fn()
    stack.on('speech-active', speech)
    internals(stack)._handleCommand(cmd('modesChanged', { appStates: 'nope' }), session)
    internals(stack)._handleCommand(
      cmd('modesChanged', { appStates: [{ appStateID: 2 }, { appStateID: 1 }] }),
      session
    )
    expect(speech).not.toHaveBeenCalled()
  })

  it('logs suggestUI without showing it', async () => {
    const { stack, session } = await fresh()
    internals(stack)._handleCommand(cmd('suggestUI', { urls: ['a', 'b'] }), session)
    internals(stack)._handleCommand(cmd('suggestUI', {}), session)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('suggestUI'))
  })

  it('relays an iAPSendMessage buffer and ignores non-buffer data', async () => {
    const { stack, session } = await fresh()
    const relay = fakeSock()
    session.iapRelay = relay
    internals(stack)._handleCommand(cmd('iAPSendMessage', { data: Buffer.from('iap') }), session)
    internals(stack)._handleCommand(cmd('iAPSendMessage', { data: 'not-a-buffer' }), session)
    expect(relay.write).toHaveBeenCalledTimes(1)
  })

  it('emits duck and unduck with computed levels', async () => {
    const { stack, session } = await fresh()
    const duck = vi.fn()
    stack.on('duck', duck)
    internals(stack)._handleCommand(cmd('duckAudio', { durationMs: 200, volume: -20 }), session)
    internals(stack)._handleCommand(cmd('unduckAudio', { durationMs: 100 }), session)
    expect(duck).toHaveBeenNthCalledWith(1, expect.any(Number), 200)
    expect(duck).toHaveBeenNthCalledWith(2, 1, 100)
  })

  it('logs an unhandled command type and tolerates a non-plist body', async () => {
    const { stack, session } = await fresh()
    internals(stack)._handleCommand(cmd('somethingNew', { a: 1 }), session)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining("unhandled command 'somethingNew'"))
    const res = internals(stack)._handleCommand(
      req('POST', 'rtsp://x/command', Buffer.from('bad')),
      session
    )
    expect(res).toMatchObject({ status: 200 })
  })
})

describe('CpStack feedback', () => {
  async function fresh(debug = false): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack(debug)
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  function pushStream(
    session: Session,
    origin: { originNs: bigint; firstSample: number } | null
  ): void {
    session.audioMeta.push({
      type: 102,
      sampleRate: 48000,
      connectionID: 7,
      playoutLatencyMs: 1000,
      hostStreamId: 5,
      micStreamId: null,
      origin
    })
  }

  it('answers 200 with no streams', async () => {
    const { stack, session } = await fresh()
    expect(internals(stack)._buildFeedback(session)).toMatchObject({ status: 200 })
  })

  it('reports a stream with a media-clock origin, using synced ntp under debug', async () => {
    const { stack, session } = await fresh(true)
    session.timing = { syncedNtp: () => 555n }
    pushStream(session, { originNs: process.hrtime.bigint() - 1_000_000_000n, firstSample: 0 })
    pushStream(session, { originNs: process.hrtime.bigint() - 1_000_000_000n, firstSample: 0 })
    pushStream(session, { originNs: process.hrtime.bigint() - 1_000_000_000n, firstSample: 0 })
    const res = internals(stack)._buildFeedback(session) as Record<string, unknown>
    expect(Buffer.isBuffer(res.body)).toBe(true)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('fb]'))
  })

  it('falls back to raw ntp for an anchored stream without a synced clock', async () => {
    const { stack, session } = await fresh()
    session.timing = null
    pushStream(session, { originNs: process.hrtime.bigint() - 1_000_000_000n, firstSample: 0 })

    const res = internals(stack)._buildFeedback(session) as Record<string, unknown>

    expect(Buffer.isBuffer(res.body)).toBe(true)
  })

  it('reports a stream without an origin and falls back to raw ntp', async () => {
    const { stack, session } = await fresh()
    session.timing = null
    pushStream(session, null)
    const res = internals(stack)._buildFeedback(session) as Record<string, unknown>
    expect(Buffer.isBuffer(res.body)).toBe(true)
  })
})

describe('CpStack HID senders', () => {
  async function fresh(): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('sends touch reports only when an event connection is active', async () => {
    const { stack, session } = await fresh()
    ;(stack as unknown as { sendTouches(c: unknown[]): void }).sendTouches([
      { id: 0, x: 0.5, y: 0.5, down: true }
    ])
    const evSock = eventReady(session)
    internals(stack)._active = session
    ;(stack as unknown as { sendTouches(c: unknown[]): void }).sendTouches([
      { id: 0, x: 0.5, y: 0.5, down: true }
    ])
    expect(evSock.write).toHaveBeenCalled()
  })

  it('sends knob, media, telephony and siri commands', async () => {
    const { stack, session } = await fresh()
    const s = stack as unknown as {
      sendKnob(state: unknown, momentary?: boolean): void
      sendKnobSelect(d: boolean): void
      sendMedia(i: number): void
      sendTelephony(i: number): void
      invokeSiri(): void
    }
    s.invokeSiri()
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('no active event connection'))
    const evSock = eventReady(session)
    internals(stack)._active = session
    s.sendKnob({ x: 1 })
    s.sendKnob({ x: 1 }, false)
    s.sendKnobSelect(true)
    s.sendMedia(3)
    s.sendTelephony(2)
    s.invokeSiri()
    expect(evSock.write).toHaveBeenCalled()
  })

  it('ignores hid reports when no event connection exists', async () => {
    const { stack } = await fresh()
    const s = stack as unknown as { sendMedia(i: number): void }
    expect(() => s.sendMedia(1)).not.toThrow()
  })
})

describe('CpStack night mode, keyframes and cluster', () => {
  async function fresh(
    cfg?: Partial<CpStackConfig>
  ): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg(cfg))
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('stores night mode and pushes it to the active event connection', async () => {
    const { stack, session } = await fresh()
    const nm = stack as unknown as { setNightMode(n: boolean): void }
    nm.setNightMode(true)
    expect(internals(stack)._nightMode).toBe(true)
    const evSock = eventReady(session)
    internals(stack)._active = session
    nm.setNightMode(false)
    expect(evSock.write).toHaveBeenCalled()
  })

  it('guards keyframe requests on the active, ready session', async () => {
    const { stack, session } = await fresh()
    const kf = stack as unknown as { forceMainKeyframe(): void; forceClusterKeyframe(): void }
    kf.forceMainKeyframe()
    kf.forceClusterKeyframe()
    const evSock = eventReady(session)
    internals(stack)._active = session
    kf.forceMainKeyframe()
    session.mainStreamReady = true
    kf.forceMainKeyframe()
    kf.forceClusterKeyframe()
    internals(stack)._clusterWantActive = true
    kf.forceClusterKeyframe()
    expect(evSock.write).toHaveBeenCalled()
  })

  it('activates and stops the cluster stream, and no-ops without a cluster display', async () => {
    const { stack, session } = await fresh()
    const cs = stack as unknown as { setClusterStreamActive(a: boolean): void }
    cs.setClusterStreamActive(true)
    const evSock = eventReady(session)
    internals(stack)._active = session
    cs.setClusterStreamActive(true)
    session.mainStreamReady = true
    cs.setClusterStreamActive(true)
    cs.setClusterStreamActive(false)
    expect(evSock.write).toHaveBeenCalled()

    const noCluster = await fresh({ cluster: undefined })
    ;(
      noCluster.stack as unknown as { setClusterStreamActive(a: boolean): void }
    ).setClusterStreamActive(true)
    expect(internals(noCluster.stack)._clusterWantActive).toBe(false)
  })

  it('applyDisplayConfig merges and setConfigRefresh stores the callback', async () => {
    const { stack } = await fresh()
    ;(
      stack as unknown as { applyDisplayConfig(c: Partial<CpStackConfig>): void }
    ).applyDisplayConfig({ deviceName: 'X' })
    const fn = vi.fn()
    ;(stack as unknown as { setConfigRefresh(f: () => void): void }).setConfigRefresh(fn)
    await internals(stack)._handle(req('GET', 'rtsp://x/info'), (await fresh()).session)
    expect(fn).not.toThrow
  })

  it('marks native receivers active or idle', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const { stack, session } = await fresh()
    session.screenNativeId = 11
    session.clusterScreenNativeId = 22
    ;(stack as unknown as { setVideoActive(a: boolean): void }).setVideoActive(true)
    expect(reg.gst.setActiveFeeder).toHaveBeenCalledWith(11, true)
    expect(reg.gst.setActiveFeeder).toHaveBeenCalledWith(22, true)
  })
})

describe('CpStack event messages and iAP relay', () => {
  async function fresh(): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('acks phone event requests and logs event responses', async () => {
    const { stack, session } = await fresh()
    const evSock = eventReady(session)
    internals(stack)._onEventMessage(session, req('GET', '/x'))
    expect(evSock.write).toHaveBeenCalled()
    internals(stack)._onEventMessage(session, {
      method: 'RTSP/1.0',
      path: '200',
      protocol: '',
      headers: {},
      body: Buffer.alloc(0)
    })
    internals(stack)._onEventMessage(session, {
      method: 'HTTP/1.1',
      path: '500',
      protocol: '',
      headers: {},
      body: Buffer.alloc(0)
    })
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('event response 500'))
  })

  it('decodes plist and non-plist event bodies', async () => {
    const { stack, session } = await fresh()
    eventReady(session)
    internals(stack)._onEventMessage(session, req('POST', '/y', encodeBplist({ a: 1 })))
    internals(stack)._onEventMessage(session, req('POST', '/y', Buffer.from('nope')))
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('event <'))
  })

  it('skips the ack when there is no event cipher', async () => {
    const { stack, session } = await fresh()
    session.eventSock = null
    session.eventCipher = null
    expect(() => internals(stack)._onEventMessage(session, req('POST', '/y'))).not.toThrow()
  })

  it('does nothing when _sendEventCommand has no live event connection', async () => {
    const { stack, session } = await fresh()
    session.eventSock = null
    session.eventCipher = null
    expect(() => internals(stack)._sendEventCommand(session, Buffer.from('x'))).not.toThrow()
  })

  it('opens the iAP message relay once and relays helper bytes to the phone', async () => {
    const { stack, session } = await fresh()
    eventReady(session)
    const relay = fakeSock()
    reg.createConnection.mockReturnValue(relay)
    session.deviceBtMac = 'AA:BB:CC:DD:EE:FF'
    internals(stack)._openIapMessageRelay(session)
    internals(stack)._openIapMessageRelay(session)
    expect(reg.createConnection).toHaveBeenCalledTimes(1)
    expect(relay.write).toHaveBeenCalledWith('tunnel ctrl-1 AA:BB:CC:DD:EE:FF\n')
    relay.emit('data', Buffer.from('from-helper'))
    relay.emit('error', new Error('relay-fail'))
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('iAP relay error'))
    relay.emit('close')
    expect(session.iapRelay).toBeNull()
  })

  it('uses an empty controller id when the session has none', async () => {
    const { stack, session } = await fresh()
    stubVerify(session, { controllerId: null })
    const relay = fakeSock()
    reg.createConnection.mockReturnValue(relay)
    internals(stack)._openIapMessageRelay(session)
    expect(relay.write).toHaveBeenCalledWith('tunnel\n')
  })

  it('omits the BT MAC from the tunnel header until the SETUP delivers one', async () => {
    const { stack, session } = await fresh()
    const relay = fakeSock()
    reg.createConnection.mockReturnValue(relay)
    internals(stack)._openIapMessageRelay(session)
    expect(relay.write).toHaveBeenCalledWith('tunnel ctrl-1\n')
  })
})

describe('CpStack audio control', () => {
  async function withStream(): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    await internals(stack)._setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 },
      session,
      100
    )
    return { stack, session }
  }

  it('sets the level of the streams of one audioType only', async () => {
    const { stack } = await withStream()

    ;(stack as unknown as CpStackControl).setStreamVolume(3, 0.5, 250)
    ;(stack as unknown as CpStackControl).setStreamVolume(2, 0.1, 0)

    expect(reg.gst.setAudioVolume).toHaveBeenCalledTimes(1)
    expect(reg.gst.setAudioVolume).toHaveBeenCalledWith(1, 0.5, 250)
  })

  it('switches every stream of the phone through to the sink and back', async () => {
    const { stack } = await withStream()
    reg.gst.setAudioActive.mockClear()

    ;(stack as unknown as CpStackControl).setAudioActive(true)
    ;(stack as unknown as CpStackControl).setAudioActive(false)

    expect(reg.gst.setAudioActive).toHaveBeenNthCalledWith(1, 1, true)
    expect(reg.gst.setAudioActive).toHaveBeenNthCalledWith(2, 1, false)
  })

  it('a start report for an unknown stream is ignored', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const started = vi.fn()
    stack.on('audio-active', started)

    reg.gst.onAudioStarted.mock.calls.at(-1)?.[0]?.(999, 0)

    expect(started).not.toHaveBeenCalled()
  })

  it('closing a stream the stack never opened still closes it in the host', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    session.audioMeta = [{ type: 100, hostStreamId: 77, micStreamId: null, origin: null }]

    internals(stack)._teardown(session)

    expect(reg.gst.closeAudio).toHaveBeenCalledWith(77)
  })

  it('a stream opened while the phone is active is switched through at once', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    ;(stack as unknown as CpStackControl).setAudioActive(true)

    await internals(stack)._setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 },
      session,
      100
    )

    expect(reg.gst.setAudioActive).toHaveBeenCalledWith(1, true)
  })

  it('an in-process stream opened while the phone is active is switched through directly', async () => {
    vi.mocked(openAudioReceiver).mockReturnValueOnce({
      streamId: 0x7c000010,
      dataPort: 5011,
      controlPort: 5012
    })
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    ;(stack as unknown as CpStackControl).setAudioActive(true)

    await internals(stack)._setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 },
      session,
      100
    )

    expect(setAudioReceiverActive).toHaveBeenCalledWith(0x7c000010, true)
    expect(reg.gst.setAudioActive).not.toHaveBeenCalled()
  })

  it('sets the level of an in-process stream directly, without going through the host', async () => {
    vi.mocked(openAudioReceiver).mockReturnValueOnce({
      streamId: 0x7c000011,
      dataPort: 5013,
      controlPort: 5014
    })
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    await internals(stack)._setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 },
      session,
      100
    )

    ;(stack as unknown as CpStackControl).setStreamVolume(3, 0.5, 250)

    expect(setAudioReceiverVolume).toHaveBeenCalledWith(0x7c000011, 0.5, 250)
    expect(reg.gst.setAudioVolume).not.toHaveBeenCalled()
  })

  it('switches an already-open in-process stream active directly through setAudioActive', async () => {
    vi.mocked(openAudioReceiver).mockReturnValueOnce({
      streamId: 0x7c000012,
      dataPort: 5015,
      controlPort: 5016
    })
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    await internals(stack)._setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 },
      session,
      100
    )

    ;(stack as unknown as CpStackControl).setAudioActive(true)

    expect(setAudioReceiverActive).toHaveBeenCalledWith(0x7c000012, true)
  })
})

describe('CpStack teardown', () => {
  async function fresh(): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  /** One audio stream as the stack keeps it after SETUP. */
  function meta(type = 100, micStreamId: number | null = null): Record<string, unknown> {
    return { type, hostStreamId: 5, micStreamId, origin: null }
  }

  it('a stream the stack opened reports itself inactive when it closes', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    await internals(stack)._setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x800000 },
      session,
      100
    )
    const inactive = vi.fn()
    stack.on('audio-active', (_p: unknown, active: boolean) => {
      if (!active) inactive()
    })

    internals(stack)._teardown(session)

    expect(inactive).toHaveBeenCalled()
  })

  it('closes every session resource in the host and reports the mic inactive', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const { stack, session } = await fresh()
    const micOff = vi.fn()
    stack.on('mic-active', micOff)
    session.heartbeat = setInterval(() => {}, 1000)
    session.timing = { stop: vi.fn() }
    session.keepAlive = { stop: vi.fn() }
    session.screen = { stop: vi.fn() }
    session.clusterScreen = { stop: vi.fn() }
    session.screenNativeId = 11
    session.clusterScreenNativeId = 22
    session.audioMeta = [meta(100, 9)]
    session.iapTunnel = { stop: vi.fn() }
    session.iapRelay = fakeSock()
    session.event = { close: vi.fn() }

    internals(stack)._teardown(session)

    expect(reg.gst.closeVideoReceiver).toHaveBeenCalledWith(11)
    expect(reg.gst.closeVideoReceiver).toHaveBeenCalledWith(22)
    expect(reg.gst.closeAudio).toHaveBeenCalledWith(5)
    expect(reg.gst.closeMic).toHaveBeenCalledWith(9)
    expect(micOff).toHaveBeenCalledWith(false, 0, 0)
    expect(session.audioMeta).toHaveLength(0)
    expect(session.iapRelay as FakeSock | null).toBeNull()
  })

  it('tears down a session-level TEARDOWN and a per-stream TEARDOWN', async () => {
    const { stack, session } = await fresh()
    session.screen = { stop: vi.fn() }
    session.audioMeta = [meta(100)]

    const perStream = internals(stack)._handleTeardown(
      req(
        'TEARDOWN',
        'rtsp://x',
        encodeBplist({ streams: [{ type: 110 }, { type: 100 }, { type: 999 }] })
      ),
      session
    )
    expect(perStream).toMatchObject({ status: 200 })
    expect(session.audioMeta).toHaveLength(0)
    expect(reg.gst.closeAudio).toHaveBeenCalledWith(5)

    const full = internals(stack)._handleTeardown(req('TEARDOWN', 'rtsp://x'), session)
    expect(full).toMatchObject({ status: 200 })
  })

  it('treats an invalid TEARDOWN body as a session teardown', async () => {
    const { stack, session } = await fresh()
    const res = internals(stack)._handleTeardown(
      req('TEARDOWN', 'rtsp://x', Buffer.from('bad')),
      session
    )
    expect(res).toMatchObject({ status: 200 })
  })

  it('closes in-process screen receivers directly, without going through the host', async () => {
    const { stack, session } = await fresh()
    session.screenNativeId = 11
    session.screenInProcess = true
    session.clusterScreenNativeId = 22
    session.clusterScreenInProcess = true

    internals(stack)._teardown(session)

    expect(closeScreenReceiver).toHaveBeenCalledWith(11)
    expect(closeScreenReceiver).toHaveBeenCalledWith(22)
    expect(reg.gst.closeVideoReceiver).not.toHaveBeenCalled()
  })

  it('a stream without a microphone reports nothing about it', async () => {
    const { stack, session } = await fresh()
    const micOff = vi.fn()
    stack.on('mic-active', (a: boolean) => {
      if (!a) micOff()
    })
    session.audioMeta = [meta(100, null)]

    internals(stack)._teardown(session)

    expect(micOff).not.toHaveBeenCalled()
    expect(reg.gst.closeMic).not.toHaveBeenCalled()
  })
})

const BIG = 9007199254740993n
function bodyBB(): Buffer {
  return encodeBplist({ big: BIG, buf: Buffer.from('ab') })
}

describe('CpStack branch completion', () => {
  async function fresh(
    debug = false,
    cfg?: Partial<CpStackConfig>
  ): Promise<{ stack: EventEmitter; session: Session }> {
    const CpStack = await loadStack(debug)
    const stack = new CpStack(baseCfg(cfg))
    const { session } = attach(stack)
    stubVerify(session)
    return { stack, session }
  }

  it('normalises an empty peer host and skips night mode on RECORD', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const sock = fakeSock()
    ;(sock as unknown as { remoteAddress: string | undefined }).remoteAddress = undefined
    ;(stack as unknown as { attachSocket(s: unknown): void }).attachSocket(sock)
    const session = [...internals(stack)._sessionSock.keys()].at(-1) as Session
    stubVerify(session)
    reg.createConnection.mockReturnValue(fakeSock())
    const active = vi.fn()
    stack.on('session-active', active)
    await internals(stack)._handle(req('RECORD', 'rtsp://x'), session)
    expect(active).toHaveBeenCalledWith('')
  })

  it('renders bigint and buffer values in debug body and info logs', async () => {
    const { stack, session } = await fresh(true)
    await internals(stack)._handle(req('GET', 'rtsp://x/other', bodyBB()), session)
    await internals(stack)._handle(req('GET', 'rtsp://x/info', bodyBB()), session)
    expect(logSpy).toHaveBeenCalledWith('[cpStack]   body:', expect.any(String))
  })

  it('reports MISSING audio arrays when audio output is disabled', async () => {
    const { stack, session } = await fresh(false, { disableAudioOutput: true })
    await internals(stack)._handle(req('GET', 'rtsp://x/info'), session)
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('audioFormats=MISSING'))
  })

  it('routes TEARDOWN through the top-level handler', async () => {
    const { stack, session } = await fresh()
    const res = await internals(stack)._handle(req('TEARDOWN', 'rtsp://x'), session)
    expect(res).toMatchObject({ status: 200 })
  })

  it('tolerates commands whose params or bodies are absent', async () => {
    const { stack, session } = await fresh(true)
    const bare = (type: string): RtspRequest =>
      req('POST', 'rtsp://x/command', encodeBplist({ type }))
    internals(stack)._handleCommand(bare('modesChanged'), session)
    internals(stack)._handleCommand(bare('disableBluetooth'), session)
    internals(stack)._handleCommand(bare('suggestUI'), session)
    session.iapRelay = fakeSock()
    internals(stack)._handleCommand(bare('iAPSendMessage'), session)
    internals(stack)._handleCommand(bare('duckAudio'), session)
    internals(stack)._handleCommand(bare('unduckAudio'), session)
    internals(stack)._handleCommand(
      req('POST', 'rtsp://x/command', encodeBplist({ type: 'weird', big: BIG })),
      session
    )
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining("unhandled command 'weird'"))
  })

  it('serialises a modesChanged bigint under debug', async () => {
    const { stack, session } = await fresh(true)
    internals(stack)._handleCommand(
      req('POST', 'rtsp://x/command', encodeBplist({ type: 'modesChanged', params: { big: BIG } })),
      session
    )
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('modesChanged'))
  })

  it('activates a native main screen without a pending cluster', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const { stack, session } = await fresh()
    await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    expect(session.mainStreamReady).toBe(true)
  })

  it('reports feedback with raw ntp and no debug logging', async () => {
    const { stack, session } = await fresh(false)
    session.timing = null
    session.audioMeta.push({
      type: 102,
      sampleRate: 48000,
      connectionID: 7,
      playoutLatencyMs: 1000,
      stream: {
        getOrigin: () => ({ originNs: process.hrtime.bigint() - 1_000_000_000n, firstSample: 0 }),
        getLastRecvSample: () => 96000
      },
      decoder: null,
      uplink: null
    })
    const res = internals(stack)._buildFeedback(session) as Record<string, unknown>
    expect(Buffer.isBuffer(res.body)).toBe(true)
    expect(logSpy).not.toHaveBeenCalledWith(expect.stringContaining('fb]'))
  })

  it('skips a foreign data stream inside a SETUP stream list', async () => {
    const { stack, session } = await fresh()
    const body = encodeBplist({ streams: [{ type: 130, clientTypeUUID: 'other' }] })
    const res = (await internals(stack)._handleSetup(
      req('SETUP', 'rtsp://x', body),
      session
    )) as Record<string, unknown>
    expect(reg.tunnels).toHaveLength(0)
    expect(Buffer.isBuffer(res.body)).toBe(true)
  })

  it('leaves a foreign controller active when another event socket closes', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    let handler: ((s: unknown) => void) | undefined
    reg.createServer.mockImplementation((h: (s: unknown) => void) => {
      handler = h
      return fakeServer()
    })
    await internals(stack)._openEventChannel(session)
    const evSock = fakeSock()
    handler?.(evSock)
    internals(stack)._active = null
    expect(() => evSock.emit('close')).not.toThrow()
  })

  it('logs an event response without a protocol', async () => {
    const { stack, session } = await fresh()
    internals(stack)._onEventMessage(session, {
      method: 'RTSP/1.0',
      path: '404',
      protocol: undefined as unknown as string,
      headers: {},
      body: Buffer.alloc(0)
    })
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('event response 404'))
  })
})

describe('CpStack audio format branches', () => {
  async function setupAudio(sd: Record<string, unknown>, type = 100): Promise<EventEmitter> {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    await internals(stack)._setupAudio(sd, session, type)
    return stack
  }

  /** The settings the stack opened its stream with. */
  function opened(): Record<string, unknown> {
    return reg.gst.openAudio.mock.calls[0][1] as Record<string, unknown>
  }

  /** The settings the stack opened its microphone with. */
  function mic(): Record<string, unknown> {
    return reg.gst.openMic.mock.calls[0][1] as Record<string, unknown>
  }

  /** Reports the first packet, which is when the microphone opens. */
  function started(stack: EventEmitter): void {
    reg.gst.onAudioStarted.mock.calls.at(-1)?.[0]?.(1, 0)
    void stack
  }

  it('labels a default audio profile as nav for a blank audio type', async () => {
    await setupAudio({ streamConnectionID: 1, audioType: '', audioFormat: 0 }, 101)
    expect(reg.gst.openAudio).toHaveBeenCalledTimes(1)
  })

  it('defaults the audio type to media when omitted', async () => {
    await setupAudio({ streamConnectionID: 1, audioFormat: 0x800000 }, 100)
    expect(opened()).toMatchObject({ codec: 'aac-lc', realtime: false })
  })

  it('picks the 48k opus tier and a 96k bitrate microphone', async () => {
    const stack = await setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x40000000, dataPort: 6000 },
      100
    )
    expect(opened()).toMatchObject({ codec: 'opus' })

    started(stack)

    expect(mic()).toMatchObject({ bitrate: 96000, codec: 'opus', sampleRate: 48000 })
  })

  it('opus always clocks at 48k, whatever tier the phone picks', async () => {
    await setupAudio({ streamConnectionID: 1, audioType: 'media', audioFormat: 0x10000000 }, 101)
    expect(opened()).toMatchObject({ clockRate: 48000 })
  })

  it('builds a pcm microphone at the 32k tier', async () => {
    const stack = await setupAudio(
      { streamConnectionID: 1, audioType: 'telephony', audioFormat: 0x100, dataPort: 6000 },
      100
    )

    started(stack)

    expect(mic()).toMatchObject({ codec: 'pcm', bitrate: 64000 })
  })

  it('a stream without a send port opens no microphone', async () => {
    const stack = await setupAudio(
      { streamConnectionID: 1, audioType: 'media', audioFormat: 0x40000000 },
      100
    )

    started(stack)

    expect(reg.gst.openMic).not.toHaveBeenCalled()
  })
})

describe('CpStack data stream and screen branches', () => {
  it('logs a data stream that carries no client uuid', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const res = await internals(stack)._setupDataStream({}, session)
    expect(res).toBeNull()
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('(no uuid)'))
  })

  it('does not clear the relay when a superseded relay socket closes', async () => {
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    eventReady(session)
    const first = fakeSock()
    reg.createConnection.mockReturnValue(first)
    internals(stack)._openIapMessageRelay(session)
    const second = fakeSock()
    session.iapRelay = second
    first.emit('close')
    expect(session.iapRelay).toBe(second)
  })

  it('opens native receivers without activating idle feeders', async () => {
    Object.defineProperty(process, 'platform', { value: 'linux', configurable: true })
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    session.mainStreamReady = true
    await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    await internals(stack)._setupScreen({ streamConnectionID: 2 }, session, true)
    expect(reg.gst.setActiveFeeder).not.toHaveBeenCalled()
  })

  it('emits main frames off linux without activating an inactive cluster', async () => {
    Object.defineProperty(process, 'platform', { value: 'darwin', configurable: true })
    const CpStack = await loadStack()
    const stack = new CpStack(baseCfg())
    const { session } = attach(stack)
    stubVerify(session)
    const frame = vi.fn()
    stack.on('video-frame', frame)
    await internals(stack)._setupScreen({ streamConnectionID: 1 }, session)
    reg.screens[0]?.emit?.('frame', Buffer.from('nal'))
    expect(frame).toHaveBeenCalled()
    expect(session.mainStreamReady).toBe(true)
  })
})
