import { EventEmitter } from 'node:events'
import {
  AudioData,
  Command,
  DongleReady,
  MediaData,
  type Message,
  NavigationData,
  Opened,
  Plugged,
  Unplugged
} from '@projection/messages'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import type { Mock } from 'vitest'
import { AaEventBridge, type AaEventBridgeDeps, type AaMediaSinkDeps } from '../AaEventBridge'
import { CH } from '../stack/constants'
import type { AAStack, AAStackConfig } from '../stack/index'

function baseCfg(over: Partial<AAStackConfig> = {}): AAStackConfig {
  return {
    huName: 'LIVI',
    videoWidth: 1280,
    videoHeight: 720,
    videoFps: 30,
    videoDpi: 140,
    displayWidth: 1280,
    displayHeight: 720,
    driverPosition: 0,
    clusterEnabled: false,
    clusterWidth: 0,
    clusterHeight: 0,
    clusterFps: 0,
    clusterDpi: 0,
    ...over
  } as AAStackConfig
}

/** A host media sink whose feed and stream announcements the test controls. */
function makeSink(over: Partial<AaMediaSinkDeps> = {}): AaMediaSinkDeps {
  return {
    feedPath: async () => '/tmp/feed',
    videoPlaneId: (cluster) => (cluster ? 2 : 1),
    primeVideo: vi.fn(),
    noteVideoStarted: vi.fn(),
    audioOutputs: () => [],
    onAudioOutput: () => () => {},
    primeAudio: vi.fn(),
    setHostVolume: vi.fn(),
    ...over
  }
}

function makeBridge(over: Partial<AaEventBridgeDeps> = {}, cfgOver?: AAStackConfig) {
  const aa = new EventEmitter() as unknown as AAStack
  const emitMessage = vi.fn<void, [Message]>()
  const emitCodec = vi.fn<void, ['video-codec' | 'cluster-video-codec', string]>()
  const emitDevicePresence = vi.fn()
  const emitDeviceStatus = vi.fn()
  const startMic = vi.fn<void, [string]>()
  const stopMic = vi.fn<void, [string]>()
  const isClosed = vi.fn<boolean, []>(() => false)
  const deps: AaEventBridgeDeps = {
    emitMessage,
    emitCodec,
    emitDevicePresence,
    emitDeviceStatus,
    startMic,
    stopMic,
    isClosed,
    ...over
  }
  const cfg = cfgOver ?? baseCfg()
  const bridge = new AaEventBridge(aa, cfg, deps)
  bridge.wire()
  return {
    aa: aa as unknown as EventEmitter,
    bridge,
    deps,
    emitMessage,
    emitCodec,
    emitDevicePresence,
    emitDeviceStatus,
    startMic,
    stopMic,
    isClosed
  }
}

/** Lets the async feed path lookups behind the sink pushes settle. */
const settle = (): Promise<void> => new Promise((r) => setImmediate(r))

function allMessages(emitMessage: Mock): Message[] {
  return emitMessage.mock.calls.map((c) => c[0] as Message)
}

function messagesOf<T extends Message>(
  emitMessage: Mock,
  cls: abstract new (...args: never[]) => T
): T[] {
  return allMessages(emitMessage).filter((m): m is T => m instanceof cls)
}

function commands(emitMessage: Mock): Command[] {
  return messagesOf(emitMessage, Command)
}

function metas(emitMessage: Mock): (MediaData | NavigationData)[] {
  return allMessages(emitMessage).filter(
    (m): m is MediaData | NavigationData => m instanceof MediaData || m instanceof NavigationData
  )
}

function asMedia(m: MediaData | NavigationData): MediaData {
  if (!(m instanceof MediaData)) throw new Error('not a media meta')
  return m
}

function asNavi(m: MediaData | NavigationData): NavigationData {
  if (!(m instanceof NavigationData)) throw new Error('not a navi meta')
  return m
}

describe('AaEventBridge', () => {
  describe('connect / disconnect lifecycle', () => {
    test('connected emits no dongle-protocol lifecycle message', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('connected')

      const msgs = allMessages(emitMessage)
      expect(msgs.some((m) => m instanceof Opened || m instanceof DongleReady)).toBe(false)
      expect(msgs.some((m) => m instanceof Plugged)).toBe(false)
    })

    test('disconnected releases video focus if it was held and emits no Unplugged', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-focus-projected')
      emitMessage.mockClear()

      aa.emit('disconnected', 'phone gone')

      const releaseEmitted = commands(emitMessage).some(
        (c) => c.value === CommandMapping.releaseVideoFocus
      )
      expect(releaseEmitted).toBe(true)
      expect(messagesOf(emitMessage, Unplugged)).toHaveLength(0)
    })

    test('disconnected without prior video focus does not emit releaseVideoFocus', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('disconnected')
      const releaseEmitted = commands(emitMessage).some(
        (c) => c.value === CommandMapping.releaseVideoFocus
      )
      expect(releaseEmitted).toBe(false)
    })
  })

  describe('device presence + status', () => {
    test('device-info forwards to emitDevicePresence', () => {
      const { aa, emitDevicePresence } = makeBridge()
      const d = { name: 'Pixel', model: 'P9', instanceId: 'i', ip: '10.0.0.2' }
      aa.emit('device-info', d)
      expect(emitDevicePresence).toHaveBeenCalledWith(d)
    })

    test('device-status forwards to emitDeviceStatus', () => {
      const { aa, emitDeviceStatus } = makeBridge()
      aa.emit('device-status', { battery: 90 })
      expect(emitDeviceStatus).toHaveBeenCalledWith({ battery: 90 })
    })
  })

  describe('audio focus ducking', () => {
    test('focusType 3 ducks to a low level, other types restore', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-focus', 3)
      aa.emit('audio-focus', 1)
      const ducks = emitMessage.mock.calls
        .map((c) => c[0] as { level?: number })
        .filter((m) => typeof m.level === 'number')
      expect(ducks.map((d) => d.level)).toEqual([0.2, 1])
    })
  })

  describe('video start', () => {
    test('video-started reports the stream geometry to the host, 1280x720 when unset', () => {
      const noteVideoStarted = vi.fn()
      const cfg = baseCfg({ videoWidth: undefined, videoHeight: undefined })
      const { aa } = makeBridge({ mediaSink: makeSink({ noteVideoStarted }) }, cfg)
      aa.emit('video-started')
      aa.emit('cluster-video-started')
      expect(noteVideoStarted.mock.calls).toEqual([
        [false, 1280, 720],
        [true, 1280, 720]
      ])
    })

    test('cluster-video-started does not re-request focus once already projected', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('cluster-video-focus-projected')
      emitMessage.mockClear()
      aa.emit('cluster-video-started')
      expect(
        commands(emitMessage).some((c) => c.value === CommandMapping.requestClusterFocus)
      ).toBe(false)
    })
  })

  describe('video focus', () => {
    test('video-focus-projected emits requestVideoFocus command', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-focus-projected')
      expect(commands(emitMessage)[0].value).toBe(CommandMapping.requestVideoFocus)
    })

    test('video-started requests focus when not already held', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-started')
      expect(commands(emitMessage).some((c) => c.value === CommandMapping.requestVideoFocus)).toBe(
        true
      )
    })

    test('video-started after a projected focus does NOT re-request it', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-focus-projected')
      emitMessage.mockClear()
      aa.emit('video-started')
      expect(commands(emitMessage).some((c) => c.value === CommandMapping.requestVideoFocus)).toBe(
        false
      )
    })

    test('cluster-video-focus-projected emits requestClusterFocus command', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('cluster-video-focus-projected')
      expect(commands(emitMessage)[0].value).toBe(CommandMapping.requestClusterFocus)
    })
  })

  describe('codec selection', () => {
    test('video-codec is forwarded via emitCodec', async () => {
      const { aa, emitCodec } = makeBridge()
      aa.emit('video-codec', 'h265')
      expect(emitCodec).toHaveBeenCalledWith('video-codec', 'h265')
    })

    test('cluster-video-codec is forwarded via emitCodec', async () => {
      const { aa, emitCodec } = makeBridge()
      aa.emit('cluster-video-codec', 'vp9')
      expect(emitCodec).toHaveBeenCalledWith('cluster-video-codec', 'vp9')
    })

    test('pushAudioSink without a media sink returns', () => {
      const { bridge } = makeBridge()
      expect(() =>
        (bridge as unknown as { pushAudioSink: (id: number, tag: string) => void }).pushAudioSink(
          1,
          'media'
        )
      ).not.toThrow()
    })

    test('the sink delivery respects closed state and empty feeds', () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
      type Deliver = {
        deliverVideoSink: (feed: string, entry: unknown) => void
        deliverAudioSink: (feed: string, ch: number, id: number) => void
      }

      const closedSend = vi.fn()
      const closed = makeBridge({ isClosed: () => true })
      Object.assign(closed.aa, { sendMediaSink: closedSend })
      ;(closed.bridge as unknown as Deliver).deliverVideoSink('/tmp/f', { ch: 1 })
      ;(closed.bridge as unknown as Deliver).deliverAudioSink('/tmp/f', 1, 10)
      expect(closedSend).not.toHaveBeenCalled()

      const openSend = vi.fn()
      const open = makeBridge({ isClosed: () => false })
      Object.assign(open.aa, { sendMediaSink: openSend })
      ;(open.bridge as unknown as Deliver).deliverVideoSink('', { ch: 1 })
      ;(open.bridge as unknown as Deliver).deliverAudioSink('/tmp/f', 1, 10)
      expect(warn).toHaveBeenCalledWith(expect.stringContaining('no media feed'))
      expect(openSend).toHaveBeenCalledTimes(2)
      warn.mockRestore()
    })

    test('a codec choice primes the host plane and hands the feed to the stack', async () => {
      const sendMediaSink = vi.fn()
      const primeVideo = vi.fn()
      const { aa } = makeBridge({ mediaSink: makeSink({ primeVideo }) })
      Object.assign(aa, { sendMediaSink })

      aa.emit('video-codec', 'h265')
      aa.emit('cluster-video-codec', 'h264')
      await settle()

      expect(primeVideo.mock.calls).toEqual([[false], [true]])
      expect(sendMediaSink.mock.calls.map((c) => c[0])).toEqual([
        { feed: '/tmp/feed', video: [{ ch: CH.VIDEO, id: 1, codec: 'h265' }] },
        { feed: '/tmp/feed', video: [{ ch: CH.CLUSTER_VIDEO, id: 2, codec: 'h264' }] }
      ])
    })
  })

  describe('audio', () => {
    test('audio-start emits an AudioData lifecycle command', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-start', 'media', 0)
      const msgs = messagesOf(emitMessage, AudioData)
      expect(msgs.length).toBeGreaterThan(0)
    })

    test('audio-setup primes one host stream per channel', async () => {
      const primeAudio = vi.fn()
      const { aa } = makeBridge({ mediaSink: makeSink({ primeAudio }) })
      aa.emit('audio-setup', 'media', 48000, 2)
      aa.emit('audio-setup', 'speech', 16000, 1)
      aa.emit('audio-setup', 'system', 16000, 1)
      expect(primeAudio.mock.calls).toEqual([
        [3, 48000, 2, 'media'],
        [4, 16000, 1, 'speech'],
        [4, 16000, 1, 'system']
      ])
    })

    test('a host stream is routed only to the channel it was tagged with', async () => {
      const sendMediaSink = vi.fn()
      let announce: ((audioType: number, streamId: number, tag?: string) => void) | undefined
      const mediaSink = makeSink({
        audioOutputs: () => [{ audioType: 3, streamId: 10, tag: 'media' }],
        onAudioOutput: (cb) => {
          announce = cb
          return () => {}
        }
      })
      const { aa } = makeBridge({ mediaSink })
      Object.assign(aa, { sendMediaSink })

      aa.emit('connected')
      announce?.(4, 11, 'speech')
      announce?.(4, 12, 'system')
      announce?.(4, 13, undefined)
      await settle()

      expect(sendMediaSink.mock.calls.map((c) => c[0].audio)).toEqual([
        [{ ch: CH.MEDIA_AUDIO, id: 10 }],
        [{ ch: CH.SPEECH_AUDIO, id: 11 }],
        [{ ch: CH.SYSTEM_AUDIO, id: 12 }]
      ])
    })

    test('audio-stop emits an AudioData lifecycle command', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-stop', 'speech', 0)
      const msgs = messagesOf(emitMessage, AudioData)
      expect(msgs.length).toBeGreaterThan(0)
    })

    test('audio-stop on the media channel emits the media-stop command', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-stop', 'media', 0)
      expect(messagesOf(emitMessage, AudioData).length).toBeGreaterThan(0)
    })
  })

  describe('microphone', () => {
    test('mic-start / mic-stop forward to deps', async () => {
      const { aa, startMic, stopMic } = makeBridge()
      aa.emit('mic-start')
      aa.emit('mic-stop')
      expect(startMic).toHaveBeenCalledWith('mic-start')
      expect(stopMic).toHaveBeenCalledWith('mic-stop')
    })

    test('voice-session active=true starts mic, active=false stops mic', async () => {
      const { aa, startMic, stopMic } = makeBridge()
      aa.emit('voice-session', true)
      aa.emit('voice-session', false)
      expect(startMic).toHaveBeenCalledWith('voice-session START')
      expect(stopMic).toHaveBeenCalledWith('voice-session END')
    })
  })

  describe('host UI', () => {
    test('host-ui-requested emits a Command(requestHostUI)', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('host-ui-requested')
      expect(commands(emitMessage)[0].value).toBe(CommandMapping.requestHostUI)
    })
  })

  describe('media metadata / status', () => {
    function mediaJson(m: MediaData | NavigationData): Record<string, unknown> {
      const p = asMedia(m).payload as { media?: Record<string, unknown> } | undefined
      return p?.media ?? {}
    }

    test('media-metadata builds a MetaData JSON with song/artist/album/duration', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('media-metadata', {
        song: 'Title',
        artist: 'Artist',
        album: 'Album',
        durationSeconds: 12
      })
      const all = metas(emitMessage)
      expect(all).toHaveLength(1)
      expect(mediaJson(all[0])).toEqual({
        MediaSongName: 'Title',
        MediaArtistName: 'Artist',
        MediaAlbumName: 'Album',
        MediaSongDuration: 12_000
      })
    })

    test('media-metadata with albumArt emits a second MetaData message', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('media-metadata', { song: 'x', albumArt: Buffer.from([1, 2, 3]) })
      expect(metas(emitMessage)).toHaveLength(2)
    })

    test('media-metadata with no recognizable fields and no albumArt emits nothing', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('media-metadata', {})
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('media-status playing=1 / paused=0', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('media-status', { state: 'playing', mediaSource: 'Spotify', playbackSeconds: 5 })
      aa.emit('media-status', { state: 'paused' })

      const all = metas(emitMessage)
      expect(mediaJson(all[0])).toMatchObject({
        MediaPlayStatus: 1,
        MediaAPPName: 'Spotify',
        MediaSongPlayTime: 5_000
      })
      expect(mediaJson(all[1])).toEqual({ MediaPlayStatus: 0 })
    })
  })

  describe('navigation', () => {
    function naviInfo(m: MediaData | NavigationData): Record<string, unknown> {
      return (asNavi(m).navi ?? {}) as Record<string, unknown>
    }

    test('nav-start sets NaviStatus=1 + NaviAPPName', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-start')
      expect(naviInfo(metas(emitMessage)[0])).toMatchObject({
        NaviStatus: 1,
        NaviAPPName: 'Google Maps'
      })
    })

    test('nav-stop sets NaviStatus=0', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-start')
      emitMessage.mockClear()
      aa.emit('nav-stop')
      expect(naviInfo(metas(emitMessage)[0]).NaviStatus).toBe(0)
    })

    test('nav-status active/rerouting → 1, idle → 0', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-status', { state: 'active' })
      aa.emit('nav-status', { state: 'idle' })
      const all = metas(emitMessage)
      expect(naviInfo(all[0]).NaviStatus).toBe(1)
      expect(naviInfo(all[1]).NaviStatus).toBe(0)
    })

    test('nav-distance maps to the maneuver distance, not the destination', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-distance', {
        distanceMeters: 500,
        timeToTurnSeconds: 30,
        displayDistanceE3: 0.5,
        displayUnit: 'km'
      })
      const info = naviInfo(metas(emitMessage)[0])
      expect(info).toMatchObject({
        NaviRemainDistance: 500,
        NaviDisplayDistanceE3: 0.5,
        NaviDisplayDistanceUnit: 'km'
      })
      // AA never carries the trip distance/ETA, so those destination fields stay unset
      expect(info.NaviDistanceToDestination).toBeUndefined()
      expect(info.NaviTimeToDestination).toBeUndefined()
    })

    test('nav-state maps the modern maneuver enum + road + destination address', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-state', {
        maneuverType: 8, // TURN_NORMAL_RIGHT
        roadName: 'Jarrestraße',
        destinationAddress: 'Harburger Ring 24, Harburg'
      })
      expect(naviInfo(metas(emitMessage)[0])).toMatchObject({
        NaviManeuverType: 2, // right
        NaviTurnSide: 0, // right
        NaviRoadName: 'Jarrestraße',
        NaviDestinationName: 'Harburger Ring 24, Harburg'
      })
    })

    test('nav-position maps step distance + destination distance + ETA', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-position', {
        stepDistanceMeters: 345,
        destinationMeters: 18185,
        timeToArrivalSeconds: 1599,
        etaText: '21:58'
      })
      expect(naviInfo(metas(emitMessage)[0])).toMatchObject({
        NaviRemainDistance: 345,
        NaviDistanceToDestination: 18185,
        NaviTimeToDestination: 1599,
        NaviETA: '21:58'
      })
    })

    test('nav-turn with image emits a separate DashboardImage MetaData', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-turn', { road: 'Main St', image: Buffer.from([1, 2, 3]) })
      // 1× dashboard-info (road update), 1× dashboard-image
      expect(metas(emitMessage)).toHaveLength(2)
    })

    test('nav-turn without any recognizable fields emits no nav-info patch', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-turn', {})
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('nav-turn maps event/side/angle/turn-number into the nav bag', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-turn', {
        road: 'Main St',
        event: 'turn',
        turnSide: 'right',
        turnAngle: 90,
        turnNumber: 2
      })
      expect(naviInfo(metas(emitMessage)[0])).toMatchObject({
        NaviRoadName: 'Main St',
        NaviManeuverType: 2,
        NaviTurnSide: 0,
        NaviTurnAngle: 90,
        NaviRoundaboutExitNumber: 2
      })
    })

    test('nav-distance without display fields carries only the raw distance', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-distance', { distanceMeters: 250, timeToTurnSeconds: 10 })
      const info = naviInfo(metas(emitMessage)[0])
      expect(info.NaviRemainDistance).toBe(250)
      expect(info.NaviDisplayDistanceE3).toBeUndefined()
      expect(info.NaviDisplayDistanceUnit).toBeUndefined()
    })

    test('nav-state with an unmapped maneuver and no road/dest publishes nothing', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-state', { maneuverType: 999 })
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('nav-state with no fields at all publishes nothing', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-state', {})
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('nav-position with no fields publishes nothing', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-position', {})
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('publishNavi injects naviApp when the bag has lost its app name', async () => {
      const { aa, bridge, emitMessage } = makeBridge()
      ;(bridge as unknown as { naviApp: string }).naviApp = 'Waze'
      ;(bridge as unknown as { naviBag: Record<string, unknown> }).naviBag = {}
      aa.emit('nav-status', { state: 'active' })
      expect(asNavi(metas(emitMessage)[0]).navi?.NaviAPPName).toBe('Waze')
    })

    test('disconnect resets the nav bag', async () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-start')
      emitMessage.mockClear()
      aa.emit('disconnected')
      aa.emit('nav-status', { state: 'idle' })
      // After reset, the new nav-status should not carry the old NaviAPPName
      expect(naviInfo(metas(emitMessage)[0]).NaviAPPName).toBeUndefined()
    })
  })

  describe('errors', () => {
    test('AAStack error during open session is logged as warning', async () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(function () {})
      const { aa } = makeBridge()
      aa.emit('error', new Error('transient'))
      expect(warn).toHaveBeenCalled()
      warn.mockRestore()
    })

    test('AAStack error during close is suppressed (debug only)', async () => {
      const warn = vi.spyOn(console, 'warn').mockImplementation(function () {})
      const debug = vi.spyOn(console, 'debug').mockImplementation(function () {})
      const { aa } = makeBridge({ isClosed: () => true })
      aa.emit('error', new Error('suppressed'))
      expect(warn).not.toHaveBeenCalled()
      expect(debug).toHaveBeenCalled()
      warn.mockRestore()
      debug.mockRestore()
    })
  })

  test('audio lifecycle command for "system" channel emits AudioNavi* commands, like CarPlay alerts', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('audio-start', 'system', 0)
    aa.emit('audio-stop', 'system', 0)
    const cmds = messagesOf(emitMessage, AudioData).map((m) => m.command)
    expect(cmds).toEqual([AudioCommand.AudioNaviStart, AudioCommand.AudioNaviStop])
  })

  test('audio lifecycle command for "speech" channel emits AudioNavi* commands', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('audio-start', 'speech', 0)
    aa.emit('audio-stop', 'speech', 0)
    expect(messagesOf(emitMessage, AudioData).length).toBeGreaterThanOrEqual(2)
  })

  test('publishNavi preserves naviApp when already present in the bag', async () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('nav-start') // sets naviApp = "Google Maps"
    emitMessage.mockClear()
    // emit a status update. publishNavi should pull naviApp from the existing bag (already set), not re-inject
    aa.emit('nav-status', { state: 'active' })
    const meta = metas(emitMessage)[0]
    expect(asNavi(meta).navi?.NaviAPPName).toBe('Google Maps')
  })
})
