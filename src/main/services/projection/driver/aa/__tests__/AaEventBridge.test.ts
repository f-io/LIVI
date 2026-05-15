import { EventEmitter } from 'node:events'
import { MessageType } from '@projection/messages/common'
import {
  AudioData,
  Command,
  DongleReady,
  type MediaData,
  type Message,
  MetaData,
  type NavigationData,
  Plugged,
  Unplugged,
  VideoData
} from '@projection/messages/readable'
import { CommandMapping } from '@shared/types/ProjectionEnums'
import { AaEventBridge, type AaEventBridgeDeps } from '../AaEventBridge'
import type { AAStack, AAStackConfig } from '../stack/index'
import type { UsbAoapBridge } from '../stack/transport/UsbAoapBridge'

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

function makeBridge(over: Partial<AaEventBridgeDeps> = {}) {
  const aa = new EventEmitter() as unknown as AAStack
  const emitMessage = jest.fn<void, [Message]>()
  const emitCodec = jest.fn<void, ['video-codec' | 'cluster-video-codec', string]>()
  const startMic = jest.fn<void, [string]>()
  const stopMic = jest.fn<void, [string]>()
  const consumeWiredBridge = jest.fn<UsbAoapBridge | null, []>(() => null)
  const isClosed = jest.fn<boolean, []>(() => false)
  const deps: AaEventBridgeDeps = {
    emitMessage,
    emitCodec,
    startMic,
    stopMic,
    consumeWiredBridge,
    isClosed,
    ...over
  }
  const bridge = new AaEventBridge(aa, baseCfg(), deps)
  bridge.wire()
  return {
    aa: aa as unknown as EventEmitter,
    bridge,
    deps,
    emitMessage,
    emitCodec,
    startMic,
    stopMic,
    consumeWiredBridge,
    isClosed
  }
}

function messagesOfType(emitMessage: jest.Mock, type: MessageType): Message[] {
  return emitMessage.mock.calls.map((c) => c[0] as Message).filter((m) => m.header?.type === type)
}

function commands(emitMessage: jest.Mock): Command[] {
  return messagesOfType(emitMessage, MessageType.Command) as Command[]
}

function metas(emitMessage: jest.Mock): MetaData[] {
  return messagesOfType(emitMessage, MessageType.MetaData) as MetaData[]
}

function asMedia(m: MetaData): MediaData {
  if (m.inner.kind !== 'media') throw new Error('not a media meta')
  return m.inner.message
}

function asNavi(m: MetaData): NavigationData {
  if (m.inner.kind !== 'navigation') throw new Error('not a navi meta')
  return m.inner.message
}

describe('AaEventBridge', () => {
  describe('connect / disconnect lifecycle', () => {
    test('connected emits DongleReady + Plugged(AndroidAuto)', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('connected')

      const types = emitMessage.mock.calls.map((c) => (c[0] as Message).header.type)
      expect(types).toContain(MessageType.Open)
      expect(types).toContain(MessageType.Plugged)
      const plugged = messagesOfType(emitMessage, MessageType.Plugged)[0] as Plugged
      expect(plugged).toBeInstanceOf(Plugged)
    })

    test('disconnected emits Unplugged + releases video focus if it was held', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-focus-projected')
      emitMessage.mockClear()

      aa.emit('disconnected', 'phone gone')

      const unplugged = messagesOfType(emitMessage, MessageType.Unplugged)
      expect(unplugged).toHaveLength(1)
      const releaseEmitted = commands(emitMessage).some(
        (c) => c.value === CommandMapping.releaseVideoFocus
      )
      expect(releaseEmitted).toBe(true)
    })

    test('disconnected without prior video focus does not emit releaseVideoFocus', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('disconnected')
      const releaseEmitted = commands(emitMessage).some(
        (c) => c.value === CommandMapping.releaseVideoFocus
      )
      expect(releaseEmitted).toBe(false)
    })

    test('watchdog disconnect consumes the wired bridge and tears it down', async () => {
      const wiredBridge = {
        forceReenum: jest.fn(async () => undefined),
        stop: jest.fn(async () => undefined)
      } as unknown as UsbAoapBridge
      const consume = jest.fn(() => wiredBridge)
      const { aa } = makeBridge({ consumeWiredBridge: consume })

      aa.emit('disconnected', 'pre-RUNNING watchdog')
      expect(consume).toHaveBeenCalledTimes(1)

      await new Promise((r) => setImmediate(r))
      expect((wiredBridge as unknown as { forceReenum: jest.Mock }).forceReenum).toHaveBeenCalled()
      expect((wiredBridge as unknown as { stop: jest.Mock }).stop).toHaveBeenCalled()
    })

    test('non-watchdog disconnect does not touch the wired bridge', () => {
      const consume = jest.fn(() => null)
      const { aa } = makeBridge({ consumeWiredBridge: consume })
      aa.emit('disconnected', 'normal')
      expect(consume).not.toHaveBeenCalled()
    })
  })

  describe('video focus', () => {
    test('video-focus-projected emits requestVideoFocus command', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-focus-projected')
      expect(commands(emitMessage)[0].value).toBe(CommandMapping.requestVideoFocus)
    })

    test('first video-frame requests focus when not already held', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-frame', Buffer.from([0, 0, 0, 1]), 0n)
      expect(commands(emitMessage).some((c) => c.value === CommandMapping.requestVideoFocus)).toBe(
        true
      )
    })

    test('subsequent video-frames do NOT re-request focus', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-focus-projected')
      emitMessage.mockClear()
      aa.emit('video-frame', Buffer.from([0]), 0n)
      expect(commands(emitMessage).some((c) => c.value === CommandMapping.requestVideoFocus)).toBe(
        false
      )
    })

    test('cluster-video-focus-projected emits requestClusterFocus command', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('cluster-video-focus-projected')
      expect(commands(emitMessage)[0].value).toBe(CommandMapping.requestClusterFocus)
    })
  })

  describe('video frames', () => {
    test('video-frame forwards a VideoData message with main MessageType', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('video-frame', Buffer.alloc(64), 0n)
      const msg = emitMessage.mock.calls
        .map((c) => c[0] as Message)
        .find((m) => m.header.type === MessageType.VideoData) as VideoData
      expect(msg).toBeInstanceOf(VideoData)
    })

    test('cluster-video-frame forwards a ClusterVideoData message', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('cluster-video-frame', Buffer.alloc(64), 0n)
      const msg = emitMessage.mock.calls
        .map((c) => c[0] as Message)
        .find((m) => m.header.type === MessageType.ClusterVideoData) as VideoData
      expect(msg).toBeInstanceOf(VideoData)
    })
  })

  describe('codec selection', () => {
    test('video-codec is forwarded via emitCodec', () => {
      const { aa, emitCodec } = makeBridge()
      aa.emit('video-codec', 'h265')
      expect(emitCodec).toHaveBeenCalledWith('video-codec', 'h265')
    })

    test('cluster-video-codec is forwarded via emitCodec', () => {
      const { aa, emitCodec } = makeBridge()
      aa.emit('cluster-video-codec', 'vp9')
      expect(emitCodec).toHaveBeenCalledWith('cluster-video-codec', 'vp9')
    })
  })

  describe('audio', () => {
    test('audio-frame emits an AudioData message', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-frame', Buffer.alloc(32), 0n, 'media', 0)
      const msg = emitMessage.mock.calls
        .map((c) => c[0] as Message)
        .find((m) => m.header.type === MessageType.AudioData) as AudioData
      expect(msg).toBeInstanceOf(AudioData)
    })

    test('audio-start emits an AudioData lifecycle command', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-start', 'media', 0)
      const msgs = messagesOfType(emitMessage, MessageType.AudioData)
      expect(msgs.length).toBeGreaterThan(0)
    })

    test('audio-stop emits an AudioData lifecycle command', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('audio-stop', 'speech', 0)
      const msgs = messagesOfType(emitMessage, MessageType.AudioData)
      expect(msgs.length).toBeGreaterThan(0)
    })
  })

  describe('microphone', () => {
    test('mic-start / mic-stop forward to deps', () => {
      const { aa, startMic, stopMic } = makeBridge()
      aa.emit('mic-start')
      aa.emit('mic-stop')
      expect(startMic).toHaveBeenCalledWith('mic-start')
      expect(stopMic).toHaveBeenCalledWith('mic-stop')
    })

    test('voice-session active=true starts mic, active=false stops mic', () => {
      const { aa, startMic, stopMic } = makeBridge()
      aa.emit('voice-session', true)
      aa.emit('voice-session', false)
      expect(startMic).toHaveBeenCalledWith('voice-session START')
      expect(stopMic).toHaveBeenCalledWith('voice-session END')
    })
  })

  describe('host UI', () => {
    test('host-ui-requested emits a Command(requestHostUI)', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('host-ui-requested')
      expect(commands(emitMessage)[0].value).toBe(CommandMapping.requestHostUI)
    })
  })

  describe('media metadata / status', () => {
    function mediaJson(m: MetaData): Record<string, unknown> {
      const p = asMedia(m).payload as { media?: Record<string, unknown> } | undefined
      return p?.media ?? {}
    }

    test('media-metadata builds a MetaData JSON with song/artist/album/duration', () => {
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

    test('media-metadata with albumArt emits a second MetaData message', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('media-metadata', { song: 'x', albumArt: Buffer.from([1, 2, 3]) })
      expect(metas(emitMessage)).toHaveLength(2)
    })

    test('media-metadata with no recognizable fields and no albumArt emits nothing', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('media-metadata', {})
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('media-status playing=1 / paused=0', () => {
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
    function naviInfo(m: MetaData): Record<string, unknown> {
      return (asNavi(m).navi ?? {}) as Record<string, unknown>
    }

    test('nav-start sets NaviStatus=1 + NaviAPPName', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-start')
      expect(naviInfo(metas(emitMessage)[0])).toMatchObject({
        NaviStatus: 1,
        NaviAPPName: 'Google Maps'
      })
    })

    test('nav-stop sets NaviStatus=0', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-start')
      emitMessage.mockClear()
      aa.emit('nav-stop')
      expect(naviInfo(metas(emitMessage)[0]).NaviStatus).toBe(0)
    })

    test('nav-status active/rerouting → 1, idle → 0', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-status', { state: 'active' })
      aa.emit('nav-status', { state: 'idle' })
      const all = metas(emitMessage)
      expect(naviInfo(all[0]).NaviStatus).toBe(1)
      expect(naviInfo(all[1]).NaviStatus).toBe(0)
    })

    test('nav-distance emits distance + time fields', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-distance', {
        distanceMeters: 500,
        timeToTurnSeconds: 30,
        displayDistanceE3: 0.5,
        displayUnit: 'km'
      })
      expect(naviInfo(metas(emitMessage)[0])).toMatchObject({
        NaviDistanceToDestination: 500,
        NaviTimeToDestination: 30,
        NaviDisplayDistanceE3: 0.5,
        NaviDisplayDistanceUnit: 'km'
      })
    })

    test('nav-turn with image emits a separate DashboardImage MetaData', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-turn', { road: 'Main St', image: Buffer.from([1, 2, 3]) })
      // 1× dashboard-info (road update), 1× dashboard-image
      expect(metas(emitMessage)).toHaveLength(2)
    })

    test('nav-turn without any recognizable fields emits no nav-info patch', () => {
      const { aa, emitMessage } = makeBridge()
      aa.emit('nav-turn', {})
      expect(metas(emitMessage)).toHaveLength(0)
    })

    test('disconnect resets the nav bag', () => {
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
    test('AAStack error during open session is logged as warning', () => {
      const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
      const { aa } = makeBridge()
      aa.emit('error', new Error('transient'))
      expect(warn).toHaveBeenCalled()
      warn.mockRestore()
    })

    test('AAStack error during close is suppressed (debug only)', () => {
      const warn = jest.spyOn(console, 'warn').mockImplementation(() => {})
      const debug = jest.spyOn(console, 'debug').mockImplementation(() => {})
      const { aa } = makeBridge({ isClosed: () => true })
      aa.emit('error', new Error('suppressed'))
      expect(warn).not.toHaveBeenCalled()
      expect(debug).toHaveBeenCalled()
      warn.mockRestore()
      debug.mockRestore()
    })
  })

  describe('emitPlugged (manual)', () => {
    test('publicly callable, emits a Plugged message', () => {
      const { bridge, emitMessage } = makeBridge()
      emitMessage.mockClear()
      bridge.emitPlugged()
      expect(messagesOfType(emitMessage, MessageType.Plugged)).toHaveLength(1)
    })
  })

  test('verifies DongleReady is the message class for the Open header', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('connected')
    const open = emitMessage.mock.calls
      .map((c) => c[0] as Message)
      .find((m) => m.header.type === MessageType.Open)
    expect(open).toBeInstanceOf(DongleReady)
  })

  test('audio lifecycle command for "phone" channel emits AudioOutput* commands', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('audio-start', 'phone', 0)
    aa.emit('audio-stop', 'phone', 0)
    expect(messagesOfType(emitMessage, MessageType.AudioData).length).toBeGreaterThanOrEqual(2)
  })

  test('audio lifecycle command for "speech" channel emits AudioNavi* commands', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('audio-start', 'speech', 0)
    aa.emit('audio-stop', 'speech', 0)
    expect(messagesOfType(emitMessage, MessageType.AudioData).length).toBeGreaterThanOrEqual(2)
  })

  test('watchdog forceReenum throwing is swallowed', async () => {
    const wiredBridge = {
      forceReenum: jest.fn(async () => {
        throw new Error('USB hung')
      }),
      stop: jest.fn(async () => undefined)
    } as unknown as import('../stack/transport/UsbAoapBridge').UsbAoapBridge
    const { aa } = makeBridge({ consumeWiredBridge: () => wiredBridge })
    aa.emit('disconnected', 'pre-RUNNING watchdog')
    await new Promise((r) => setImmediate(r))
    await new Promise((r) => setImmediate(r))
    expect((wiredBridge as unknown as { stop: jest.Mock }).stop).toHaveBeenCalled()
  })

  test('watchdog bridge.stop throwing is swallowed', async () => {
    const wiredBridge = {
      forceReenum: jest.fn(async () => undefined),
      stop: jest.fn(async () => {
        throw new Error('hung')
      })
    } as unknown as import('../stack/transport/UsbAoapBridge').UsbAoapBridge
    const { aa } = makeBridge({ consumeWiredBridge: () => wiredBridge })
    aa.emit('disconnected', 'pre-RUNNING watchdog')
    await new Promise((r) => setImmediate(r))
    await new Promise((r) => setImmediate(r))
    expect((wiredBridge as unknown as { forceReenum: jest.Mock }).forceReenum).toHaveBeenCalled()
  })

  test('publishNavi preserves naviApp when already present in the bag', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('nav-start') // sets naviApp = "Google Maps"
    emitMessage.mockClear()
    // emit a status update — publishNavi should pull naviApp from the existing bag (already set), not re-inject
    aa.emit('nav-status', { state: 'active' })
    const meta = metas(emitMessage)[0]
    expect(asNavi(meta).navi?.NaviAPPName).toBe('Google Maps')
  })

  test('Unplugged message class on disconnect', () => {
    const { aa, emitMessage } = makeBridge()
    aa.emit('disconnected')
    const u = emitMessage.mock.calls
      .map((c) => c[0] as Message)
      .find((m) => m.header.type === MessageType.Unplugged)
    expect(u).toBeInstanceOf(Unplugged)
  })
})
