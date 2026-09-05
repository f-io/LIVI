import { EventEmitter } from 'node:events'
import type net from 'node:net'
import { InputCommand } from '@main/shared/types/InputCommand'
import type { Config } from '@shared/types'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { AudioData, Command, DuckAudio } from '../../../messages/readable'
import { SendableMessage, SendCommand, SendMultiTouch, SendTouch } from '../../../messages/sendable'
import { CpSession } from '../CpSession'
import { MediaButton, TelephonyButton } from '../stack/hid'

const { StackMock, stackInstances, panelMock, wifiMock, btMock } = vi.hoisted(() => {
  const stackInstances: Record<string, unknown>[] = []

  class StackMock {
    cfg: unknown
    activeControllerId: string | null = null
    private readonly _l: Record<string, ((...a: unknown[]) => void)[]> = {}
    attachSocket = vi.fn()
    setConfigRefresh = vi.fn()
    applyDisplayConfig = vi.fn()
    setNightMode = vi.fn()
    setClusterStreamActive = vi.fn()
    setVideoActive = vi.fn()
    setAudioActive = vi.fn()
    setStreamVolume = vi.fn()
    forceMainKeyframe = vi.fn()
    forceClusterKeyframe = vi.fn()
    sendTouches = vi.fn()
    sendMedia = vi.fn()
    sendKnob = vi.fn()
    sendKnobSelect = vi.fn()
    sendTelephony = vi.fn()
    invokeSiri = vi.fn()
    writeMic = vi.fn()
    stop = vi.fn()
    constructor(cfg: unknown) {
      this.cfg = cfg
      stackInstances.push(this as unknown as Record<string, unknown>)
    }
    on(ev: string, cb: (...a: unknown[]) => void): this {
      ;(this._l[ev] ||= []).push(cb)
      return this
    }
    fire(ev: string, ...args: unknown[]): void {
      for (const f of this._l[ev] || []) f(...args)
    }
  }

  return {
    StackMock,
    stackInstances,
    panelMock: vi.fn(),
    wifiMock: vi.fn(),
    btMock: vi.fn()
  }
})

vi.mock('../stack/cpStack', () => ({ CpStack: StackMock }))
vi.mock('@main/services/video/GstVideo', () => ({ panelPhysicalMm: panelMock }))
vi.mock('../../aa/stack/system/hwaddr', () => ({
  detectBtMac: btMock,
  detectWifiBssid: wifiMock
}))

type Stack = InstanceType<typeof StackMock>

function baseConfig(over: Partial<Config> = {}): Config {
  return {
    projectionWidth: 1920,
    projectionHeight: 1080,
    clusterWidth: 1280,
    clusterHeight: 720,
    projectionFps: 60,
    carName: 'Auto',
    oemName: 'OEM',
    carPlaySourceVersion: '1.0',
    samplingFrequency: 1,
    disableAudioOutput: false,
    dashboards: { dash3: { main: true }, dash4: null },
    ...over
  } as unknown as Config
}

function fakeSocket(remote = 'fe80::1%en0'): net.Socket {
  const s = new EventEmitter() as unknown as net.Socket & { remoteAddress: string }
  s.remoteAddress = remote
  return s as unknown as net.Socket
}

function makeHelper(): { disconnectBt: ReturnType<typeof vi.fn> } {
  return { disconnectBt: vi.fn(() => Promise.resolve()) }
}

function makeSession(opts?: {
  config?: Config
  socket?: net.Socket
  initialNightMode?: boolean | undefined
  clusterStreamActive?: boolean
  hevc?: boolean
}): { session: CpSession; stack: Stack; helper: ReturnType<typeof makeHelper> } {
  const helper = makeHelper()
  const session = new CpSession({
    socket: opts?.socket,
    getConfig: () => opts?.config ?? baseConfig(),
    helper: helper as never,
    seed: {
      hevcSupported: opts?.hevc ?? true,
      initialNightMode: opts?.initialNightMode,
      clusterStreamActive: opts?.clusterStreamActive ?? true
    }
  })
  const stack = stackInstances.at(-1) as unknown as Stack
  return { session, stack, helper }
}

let logSpy: ReturnType<typeof vi.spyOn>
let warnSpy: ReturnType<typeof vi.spyOn>

beforeEach(() => {
  stackInstances.length = 0
  panelMock.mockReturnValue({ widthMm: 100, heightMm: 60 })
  wifiMock.mockReturnValue('WF:00:11:22:33:44')
  btMock.mockReturnValue('BT:00:11:22:33:44')
  logSpy = vi.spyOn(console, 'log').mockImplementation(() => {})
  warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('CpSession construction and stack config', () => {
  it('builds a stack, wires the config refresh and attaches a socket', () => {
    const socket = fakeSocket()
    const { session, stack } = makeSession({ socket })
    expect(stack.setConfigRefresh).toHaveBeenCalled()
    expect(stack.attachSocket).toHaveBeenCalledWith(socket)
    expect(session.peerIp).toBe('fe80::1')
    const refresh = stack.setConfigRefresh.mock.calls[0][0] as () => void
    refresh()
    expect(stack.applyDisplayConfig).toHaveBeenCalled()
  })

  it('pushes the initial night mode when defined', () => {
    const { stack } = makeSession({ initialNightMode: true })
    expect(stack.setNightMode).toHaveBeenCalledWith(true)
  })

  it('does not push night mode when the seed leaves it undefined', () => {
    const { stack } = makeSession({ initialNightMode: undefined })
    expect(stack.setNightMode).not.toHaveBeenCalled()
  })

  it('falls back to defaults for names, macs, source version and panels', () => {
    panelMock.mockReturnValue(null)
    wifiMock.mockReturnValue(undefined)
    btMock.mockReturnValue(undefined)
    const cfg = baseConfig({
      carName: '   ',
      oemName: '   ',
      carPlaySourceVersion: '   ',
      dashboards: null,
      samplingFrequency: 0,
      projectionWidth: 0,
      projectionHeight: 0,
      projectionFps: 0
    } as Partial<Config>)
    const { stack } = makeSession({ config: cfg, hevc: false })
    const built = stack.cfg as Record<string, unknown>
    expect(built.deviceName).toBe('LIVI')
    expect(built.oemLabel).toBe('LIVI')
    expect(built.deviceId).toBe('AA:BB:CC:DD:EE:FF')
    expect(built.btMac).toBe('AA:BB:CC:DD:EE:FF')
    expect(built.h264).toBe(true)
    expect(built.cluster).toBeUndefined()
    expect(built.entertainmentSampleRate).toBe(44100)
  })

  it('includes the cluster display and physical panel sizes when configured', () => {
    const { stack } = makeSession({ config: baseConfig() })
    const built = stack.cfg as Record<string, unknown>
    expect(built.cluster).toBeDefined()
    expect((built.phoneBtMac as () => unknown)()).toBe('')
    expect((built.main as Record<string, unknown>).widthPhysicalMm).toBe(100)
    expect(built.entertainmentSampleRate).toBe(48000)
  })

  it('drops icons that decode to empty and keeps the rest', () => {
    const { stack } = makeSession({
      config: baseConfig({ dongleIcon120: 'A' } as Partial<Config>)
    })
    const icons = (stack.cfg as Record<string, unknown>).icons as unknown[]
    expect(icons).toHaveLength(2)
  })
})

describe('CpSession driver surface', () => {
  it('start resolves true and reports wireless mode', async () => {
    const { session } = makeSession()
    await expect(session.start(baseConfig())).resolves.toBe(true)
    expect(session.isWiredMode()).toBe(false)
  })

  it('codec toggles and no-op vp9/av1 setters', () => {
    const { session } = makeSession()
    expect(() => {
      session.setHevcSupported(false)
      session.setVp9Supported(true)
      session.setAv1Supported(true)
    }).not.toThrow()
  })

  it('night-mode setters reach the stack only when defined', () => {
    const { session, stack } = makeSession()
    session.setInitialNightMode(undefined)
    expect(stack.setNightMode).not.toHaveBeenCalled()
    session.setInitialNightMode(false)
    expect(stack.setNightMode).toHaveBeenCalledWith(false)
    session.sendNightMode(true)
    expect(stack.setNightMode).toHaveBeenCalledWith(true)
  })

  it('cluster, keyframe and video-active forward to the stack', () => {
    const { session, stack } = makeSession()
    session.setClusterStreamActive(true)
    expect(stack.setClusterStreamActive).toHaveBeenCalledWith(true)
    session.setStreamVolume(3, 0.5, 250)
    expect(stack.setStreamVolume).toHaveBeenCalledWith(3, 0.5, 250)
    session.requestKeyframe()
    expect(stack.forceMainKeyframe).toHaveBeenCalled()
    expect(stack.forceClusterKeyframe).toHaveBeenCalled()
    session.setVideoActive(true)
    expect(stack.setVideoActive).toHaveBeenCalledWith(true)
  })

  it('returns the controller id from the stack, or null', () => {
    const { session, stack } = makeSession()
    expect(session.getControllerId()).toBeNull()
    stack.activeControllerId = 'cid-9'
    expect(session.getControllerId()).toBe('cid-9')
  })
})

describe('CpSession identity', () => {
  it('adopts a helper device, updating mac and udid and emitting presence', () => {
    const { session } = makeSession()
    const seen: Record<string, unknown>[] = []
    session.on('device-presence', (p) => seen.push(p))
    session.adoptHelperDevice({ btMac: 'AA:BB', usbUdid: 'UDID-1', name: 'iPhone' })
    expect(session.getBtMac()).toBe('AA:BB')
    expect(seen.at(-1)).toMatchObject({ kind: 'device', btMac: 'AA:BB', usbUdid: 'UDID-1' })
  })

  it('adopts a device with only an ip without touching mac or udid', () => {
    const { session } = makeSession()
    const seen: Record<string, unknown>[] = []
    session.on('device-presence', (p) => seen.push(p))
    session.adoptHelperDevice({ ip: '10.0.0.2' })
    expect(session.getBtMac()).toBe('')
    expect(seen.at(-1)).toMatchObject({ ip: '10.0.0.2', usbUdid: '', name: '' })
  })

  it('matches on each identity axis and rejects mismatches', () => {
    const socket = fakeSocket('10.0.0.7')
    const { session, stack } = makeSession({ socket })
    stack.fire('device-info', { deviceId: 'AA:BB', wifiMac: 'CC:DD', name: 'n', model: 'm' })
    session.adoptHelperDevice({ usbUdid: 'UDID-5' })
    stack.activeControllerId = 'cid-3'

    expect(session.matchesIdentity({ btMac: 'aa:bb' })).toBe(true)
    expect(session.matchesIdentity({ wifiMac: 'cc:dd' })).toBe(true)
    expect(session.matchesIdentity({ ip: '10.0.0.7' })).toBe(true)
    expect(session.matchesIdentity({ usbUdid: 'UDID-5' })).toBe(true)
    expect(session.matchesIdentity({ controllerId: 'cid-3' })).toBe(true)
    expect(session.matchesIdentity({ btMac: 'ff:ff' })).toBe(false)
    expect(session.matchesIdentity({})).toBe(false)
  })

  it('keeps the existing btMac when device-info omits a device id', () => {
    const { session, stack } = makeSession()
    session.adoptHelperDevice({ btMac: 'AA:BB' })
    stack.fire('device-info', { wifiMac: 'CC:DD' })
    expect(session.getBtMac()).toBe('AA:BB')
  })
})

describe('CpSession stack event bridge', () => {
  it('logs stack errors', () => {
    const { stack } = makeSession()
    stack.fire('error', new Error('kaboom'))
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('stack error: kaboom'))
  })

  it('bridges session-active into an active presence', () => {
    const { session, stack } = makeSession()
    const seen: Record<string, unknown>[] = []
    session.on('device-presence', (p) => seen.push(p))
    stack.fire('session-active', '10.0.0.3')
    expect(seen.at(-1)).toMatchObject({ kind: 'active', ip: '10.0.0.3' })
  })

  it('emits identity and presence on device-info', () => {
    const { session, stack } = makeSession()
    const identity = vi.fn()
    const presence: Record<string, unknown>[] = []
    session.on('identity', identity)
    session.on('device-presence', (p) => presence.push(p))
    stack.fire('device-info', { name: 'iPhone', deviceId: 'AA:BB', wifiMac: 'CC:DD', model: 'm' })
    expect(identity).toHaveBeenCalledWith({ btMac: 'AA:BB', wifiMac: 'CC:DD' })
    expect(presence.at(-1)).toMatchObject({ kind: 'device', name: 'iPhone', model: 'm' })
  })

  it('defaults missing device-info fields to empty strings', () => {
    const { session, stack } = makeSession()
    const presence: Record<string, unknown>[] = []
    session.on('device-presence', (p) => presence.push(p))
    stack.fire('device-info', { deviceId: 'AA:BB' })
    expect(presence.at(-1)).toMatchObject({ name: '', model: '', wifiMac: '' })
  })

  it('re-emits codec and config events', () => {
    const { session, stack } = makeSession()
    const vcodec = vi.fn()
    const vconfig = vi.fn()
    const ccodec = vi.fn()
    const cconfig = vi.fn()
    session.on('video-codec', vcodec)
    session.on('video-config', vconfig)
    session.on('cluster-video-codec', ccodec)
    session.on('cluster-video-config', cconfig)
    stack.fire('video-codec', 'h265')
    stack.fire('video-config', Buffer.from('cfg'))
    stack.fire('cluster-video-codec', 'h264')
    stack.fire('cluster-video-config', Buffer.from('ccfg'))
    expect(vcodec).toHaveBeenCalledWith('h265')
    expect(vconfig).toHaveBeenCalledWith(Buffer.from('cfg'))
    expect(ccodec).toHaveBeenCalledWith('h264')
    expect(cconfig).toHaveBeenCalledWith(Buffer.from('ccfg'))
  })

  it('wraps audio commands and duck into messages', () => {
    const { session, stack } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    const prof = {
      sampleRate: 48000,
      channels: 2,
      audioType: 3,
      decodeType: 4,
      startCmd: 1,
      stopCmd: 2,
      label: 'media'
    }
    stack.fire('audio-active', prof, true)
    stack.fire('duck', 0.5, 250)
    expect(msgs[0]).toBeInstanceOf(AudioData)
    expect(msgs[1]).toBeInstanceOf(DuckAudio)
  })

  it('emits a command message when the host UI is requested', () => {
    const { session, stack } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    stack.fire('host-ui-requested')
    expect(msgs[0]).toBeInstanceOf(Command)
  })

  it('emits speech active and idle command messages', () => {
    const { session, stack } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    stack.fire('speech-active', true)
    stack.fire('speech-active', false)
    expect(msgs).toHaveLength(2)
    expect(msgs[0]).toBeInstanceOf(Command)
  })

  it('disconnects bluetooth for a trimmed device id, ignoring blanks', async () => {
    const { stack, helper } = makeSession()
    stack.fire('disable-bluetooth', '   ')
    expect(helper.disconnectBt).not.toHaveBeenCalled()
    stack.fire('disable-bluetooth', ' AA:BB ')
    expect(helper.disconnectBt).toHaveBeenCalledWith('AA:BB')
    await Promise.resolve()
    await Promise.resolve()
    expect(logSpy).toHaveBeenCalledWith(expect.stringContaining('BT disconnected'))
  })

  it('warns when disconnectBt rejects', async () => {
    const helper = makeHelper()
    helper.disconnectBt.mockRejectedValue(new Error('bt-fail'))
    const session = new CpSession({
      getConfig: () => baseConfig(),
      helper: helper as never,
      seed: { hevcSupported: true, initialNightMode: undefined, clusterStreamActive: true }
    })
    const stack = stackInstances.at(-1) as unknown as Stack
    void session
    stack.fire('disable-bluetooth', 'AA:BB')
    await Promise.resolve()
    await Promise.resolve()
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('disconnectBt failed'))
  })

  it('marks connected once on main-screen-ready', () => {
    const { session, stack } = makeSession()
    const connected = vi.fn()
    session.on('connected', connected)
    stack.fire('main-screen-ready')
    stack.fire('main-screen-ready')
    expect(connected).toHaveBeenCalledTimes(1)
  })

  it('bridges the cluster codec event', () => {
    const { session, stack } = makeSession()
    const codec = vi.fn()
    session.on('cluster-video-codec', codec)
    stack.fire('cluster-video-codec', 'h265')
    expect(codec).toHaveBeenCalledWith('h265')
  })

  it('ends the session exactly once on session-ended', () => {
    const { session, stack } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    stack.fire('session-ended')
    stack.fire('session-ended')
    expect(down).toHaveBeenCalledTimes(1)
  })
})

describe('CpSession send', () => {
  it('sends single and multi touch contacts', async () => {
    const { session, stack } = makeSession()
    await session.send(new SendTouch(0.5, 0.5, 0))
    expect(stack.sendTouches).toHaveBeenCalledWith([{ id: 0, x: 0.5, y: 0.5, down: true }])
    await session.send(new SendMultiTouch([{ id: 0, x: 2, y: -1, action: 0 }]))
    expect(stack.sendTouches).toHaveBeenLastCalledWith([{ id: 0, x: 1, y: 0, down: false }])
  })

  it('returns false for an unhandled message type', async () => {
    const { session } = makeSession()
    await expect(session.send(new (class extends SendableMessage {})())).resolves.toBe(false)
  })

  it('returns false once the stack is gone', async () => {
    const { session } = makeSession()
    await session.close()
    await expect(session.send(new SendTouch(0, 0, 0))).resolves.toBe(false)
    await expect(session.send(new SendCommand('play'))).resolves.toBe(false)
  })

  it('maps remote input commands onto the media and telephony reports', () => {
    const { session, stack } = makeSession()
    session.handleInput(InputCommand.Pause)
    expect(stack.sendMedia).toHaveBeenCalledWith(MediaButton.pause)
    session.handleInput(InputCommand.Next)
    expect(stack.sendMedia).toHaveBeenCalledWith(MediaButton.next)
    session.handleInput(InputCommand.AcceptCall)
    expect(stack.sendTelephony).toHaveBeenCalledWith(TelephonyButton.hookSwitch)
    session.handleInput(InputCommand.VoiceAssistant)
    expect(stack.invokeSiri).toHaveBeenCalled()
    stack.sendMedia.mockClear()
    session.handleInput(InputCommand.VolumeUp)
    expect(stack.sendMedia).not.toHaveBeenCalled()
  })

  it('routes every command mapping to the stack', () => {
    const { session, stack } = makeSession()
    const call = (cmd: CommandMapping): boolean =>
      (session as unknown as { _sendCommand: (c: CommandMapping) => boolean })._sendCommand(cmd)

    for (const cmd of [
      CommandMapping.play,
      CommandMapping.pause,
      CommandMapping.playPause,
      CommandMapping.next,
      CommandMapping.prev,
      CommandMapping.left,
      CommandMapping.right,
      CommandMapping.up,
      CommandMapping.down,
      CommandMapping.selectDown,
      CommandMapping.knobDown,
      CommandMapping.selectUp,
      CommandMapping.knobUp,
      CommandMapping.back,
      CommandMapping.home,
      CommandMapping.knobLeft,
      CommandMapping.knobRight,
      CommandMapping.acceptPhone,
      CommandMapping.rejectPhone,
      CommandMapping.phoneKey0,
      CommandMapping.phoneKey1,
      CommandMapping.phoneKey2,
      CommandMapping.phoneKey3,
      CommandMapping.phoneKey4,
      CommandMapping.phoneKey5,
      CommandMapping.phoneKey6,
      CommandMapping.phoneKey7,
      CommandMapping.phoneKey8,
      CommandMapping.phoneKey9,
      CommandMapping.phoneKeyStar,
      CommandMapping.phoneKeyHash,
      CommandMapping.phoneKeyHookSwitch,
      CommandMapping.voiceAssistant,
      CommandMapping.voiceAssistantRelease
    ]) {
      expect(call(cmd)).toBe(true)
    }

    expect(stack.sendMedia).toHaveBeenCalledWith(MediaButton.play)
    expect(stack.sendKnob).toHaveBeenCalledWith({ x: -127 })
    expect(stack.sendKnob).toHaveBeenCalledWith({ x: 127 })
    expect(stack.sendKnob).toHaveBeenCalledWith({ y: -127 })
    expect(stack.sendKnob).toHaveBeenCalledWith({ y: 127 })
    expect(stack.sendKnob).toHaveBeenCalledWith({ back: true })
    expect(stack.sendKnob).toHaveBeenCalledWith({ home: true })
    expect(stack.sendKnob).toHaveBeenCalledWith({ wheel: -1 })
    expect(stack.sendKnob).toHaveBeenCalledWith({ wheel: 1 })
    expect(stack.sendKnobSelect).toHaveBeenCalledWith(true)
    expect(stack.sendKnobSelect).toHaveBeenCalledWith(false)
    expect(stack.sendTelephony).toHaveBeenCalledWith(TelephonyButton.hookSwitch)
    expect(stack.sendTelephony).toHaveBeenCalledWith(TelephonyButton.drop)
    expect(stack.sendTelephony).toHaveBeenCalledWith(TelephonyButton.star)
    expect(stack.sendTelephony).toHaveBeenCalledWith(TelephonyButton.pound)
    expect(stack.invokeSiri).toHaveBeenCalled()
  })

  it('returns false for an unknown command', () => {
    const { session } = makeSession()
    const call = (session as unknown as { _sendCommand: (c: number) => boolean })._sendCommand
    expect(call.call(session, 999999 as CommandMapping)).toBe(false)
  })

  it('handleInput is a no-op', () => {
    const { session } = makeSession()
    expect(() => session.handleInput({} as never)).not.toThrow()
  })
})

describe('CpSession helper event ingest', () => {
  it('emits an album-art message for non-empty data and ignores empty', () => {
    const { session } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    session.ingestHelperEvent({ type: 'albumart', dataB64: Buffer.from('img').toString('base64') })
    session.ingestHelperEvent({ type: 'albumart', dataB64: '' })
    session.ingestHelperEvent({ type: 'albumart' })
    expect(msgs).toHaveLength(1)
  })

  it('emits power and cellular status presence', () => {
    const { session } = makeSession()
    const seen: Record<string, unknown>[] = []
    session.on('device-presence', (p) => seen.push(p))
    session.ingestHelperEvent({ type: 'power', level: 80, charging: true })
    session.ingestHelperEvent({ type: 'cellular', signal: 3, carrier: 'Carrier' })
    expect(seen[0]).toMatchObject({ kind: 'status', batteryLevel: 80, batteryCharging: true })
    expect(seen[1]).toMatchObject({ kind: 'status', signalStrength: 3, carrierName: 'Carrier' })
  })

  it('emits call-state audio commands for each phase', () => {
    const { session } = makeSession()
    const msgs: AudioData[] = []
    session.on('message', (m) => msgs.push(m as AudioData))
    session.ingestHelperEvent({ type: 'call', phase: 'ringing' })
    session.ingestHelperEvent({ type: 'call', phase: 'active' })
    session.ingestHelperEvent({ type: 'call', phase: 'ended' })
    session.ingestHelperEvent({ type: 'call', phase: 'other' })
    expect(msgs).toHaveLength(3)
    expect(msgs[0].command).toBe(AudioCommand.AudioAttentionRinging)
  })

  it('builds a media-json message from nowplaying fields', () => {
    const { session } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    session.ingestHelperEvent({
      type: 'nowplaying',
      title: 'T',
      artist: 'A',
      album: 'Al',
      appName: 'App',
      durationMs: 200,
      elapsedMs: 10,
      playing: 1
    })
    expect(msgs).toHaveLength(1)
  })

  it('ignores nowplaying with no recognised fields and other event types', () => {
    const { session } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    session.ingestHelperEvent({ type: 'nowplaying' })
    session.ingestHelperEvent({ type: 'unknown' })
    expect(msgs).toHaveLength(0)
  })

  it('builds a navigation message from all fields including eta', () => {
    const { session } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    session.ingestHelperEvent({
      type: 'navigation',
      status: 0,
      orderType: 2,
      roadName: 'Main',
      afterRoadName: 'Second',
      destinationName: 'Home',
      timeToDestination: 600,
      distanceToDestination: 5000,
      remainDistance: 100,
      maneuverType: 1,
      turnSide: 1,
      junctionType: 2,
      turnAngle: 90,
      etaEpoch: 1700000000
    })
    expect(msgs).toHaveLength(1)
  })

  it('maps a non-zero navigation status and skips a zero eta', () => {
    const { session } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    session.ingestHelperEvent({ type: 'navigation', status: 5, etaEpoch: 0 })
    expect(msgs).toHaveLength(1)
  })

  it('ignores a navigation event without any fields', () => {
    const { session } = makeSession()
    const msgs: unknown[] = []
    session.on('message', (m) => msgs.push(m))
    session.ingestHelperEvent({ type: 'navigation' })
    expect(msgs).toHaveLength(0)
  })
})

describe('CpSession branch completion', () => {
  it('emits a stop audio command when a stream goes inactive', () => {
    const { session, stack } = makeSession()
    const msgs: AudioData[] = []
    session.on('message', (m) => msgs.push(m as AudioData))
    const prof = {
      sampleRate: 48000,
      channels: 2,
      audioType: 3,
      decodeType: 4,
      startCmd: 10,
      stopCmd: 20,
      label: 'media'
    }
    stack.fire('audio-active', prof, false)
    expect(msgs[0].command).toBe(20)
  })

  it('normalises a missing peer address to an empty ip', () => {
    const socket = fakeSocket()
    ;(socket as unknown as { remoteAddress: string | undefined }).remoteAddress = undefined
    const { session } = makeSession({ socket })
    expect(session.peerIp).toBe('')
  })

  it('drops mistyped power and cellular fields', () => {
    const { session } = makeSession()
    const seen: Record<string, unknown>[] = []
    session.on('device-presence', (p) => seen.push(p))
    session.ingestHelperEvent({ type: 'power', level: 'x', charging: 'y' })
    session.ingestHelperEvent({ type: 'cellular', signal: 'x', carrier: 5 })
    expect(seen[0]).toMatchObject({ batteryLevel: undefined, batteryCharging: undefined })
    expect(seen[1]).toMatchObject({ signalStrength: undefined, carrierName: undefined })
  })

  it('sends a command through the public send path', async () => {
    const { session, stack } = makeSession()
    await expect(session.send(new SendCommand('play'))).resolves.toBe(true)
    expect(stack.sendMedia).toHaveBeenCalledWith(MediaButton.play)
  })

  it('_sendCommand returns false without a stack', async () => {
    const { session } = makeSession()
    await session.close()
    const call = (session as unknown as { _sendCommand: (c: CommandMapping) => boolean })
      ._sendCommand
    expect(call.call(session, CommandMapping.play)).toBe(false)
  })

  it('handles a cluster display whose physical panel size is unknown', () => {
    panelMock.mockReturnValue(null)
    const { stack } = makeSession({
      config: baseConfig({ projectionFps: 0 } as Partial<Config>)
    })
    const built = stack.cfg as Record<string, unknown>
    const cluster = built.cluster as Record<string, unknown>
    expect(cluster).toBeDefined()
    expect(cluster.widthPhysicalMm).toBeUndefined()
    expect(cluster.fps).toBe(60)
  })
})

describe('CpSession close', () => {
  it('tears down once, stopping the stack', async () => {
    const { session, stack } = makeSession()
    const down = vi.fn()
    session.on('disconnected', down)
    await session.close()
    await session.close()
    expect(stack.stop).toHaveBeenCalledTimes(1)
    expect(down).toHaveBeenCalledTimes(1)
  })

  it('warns when the stack stop throws', async () => {
    const { session, stack } = makeSession()
    stack.stop.mockImplementation(() => {
      throw new Error('stop boom')
    })
    await session.close()
    expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('stack stop threw'))
  })

  it('auto-closes after the disconnected event fires', async () => {
    const { session, stack } = makeSession()
    stack.fire('session-ended')
    await new Promise((r) => setImmediate(r))
    expect(stack.stop).toHaveBeenCalled()
    void session
  })
})
