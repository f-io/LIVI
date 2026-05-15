import { MessageHeader, MessageType } from '@projection/messages/common'
import {
  AudioData,
  Command,
  DongleReady,
  MediaType,
  type Message,
  MetaData,
  NavigationMetaType,
  PhoneType,
  Plugged,
  Unplugged,
  VideoData
} from '@projection/messages/readable'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import { turnEventToManeuverType, turnSideToNaviCode } from './stack/channels/navManeuverMap'
import {
  type AAStack,
  type AAStackConfig,
  type AudioChannelType,
  type MediaPlaybackMetadata,
  type MediaPlaybackStatus,
  type NavigationDistanceUpdate,
  type NavigationStatusUpdate,
  type NavigationTurnUpdate,
  type VideoCodec
} from './stack/index'
import type { UsbAoapBridge } from './stack/transport/UsbAoapBridge'

const AUDIO_MAP: Record<AudioChannelType, { audioType: number; decodeType: number }> = {
  media: { audioType: 3, decodeType: 4 },
  speech: { audioType: 1, decodeType: 5 },
  phone: { audioType: 2, decodeType: 5 }
}

function buildVideoDataMessage(
  buf: Buffer,
  width: number,
  height: number,
  type: MessageType = MessageType.VideoData
): VideoData {
  const HEADER = 20
  const data = Buffer.allocUnsafeSlow(HEADER + buf.length)
  data.writeUInt32LE(width, 0)
  data.writeUInt32LE(height, 4)
  data.writeUInt32LE(0, 8)
  data.writeUInt32LE(buf.length, 12)
  data.writeUInt32LE(0, 16)
  buf.copy(data, HEADER)
  const header = new MessageHeader(data.length, type)
  return new VideoData(header, data)
}

function buildAudioDataMessage(buf: Buffer, channel: AudioChannelType): AudioData {
  const { audioType, decodeType } = AUDIO_MAP[channel]
  const HEADER = 12
  const sampleBytes = buf.length - (buf.length % 2)
  const data = Buffer.allocUnsafeSlow(HEADER + sampleBytes)
  data.writeUInt32LE(decodeType, 0)
  data.writeFloatLE(0, 4)
  data.writeUInt32LE(audioType, 8)
  buf.copy(data, HEADER, 0, sampleBytes)
  const header = new MessageHeader(data.length, MessageType.AudioData)
  return new AudioData(header, data)
}

function buildAudioCommandMessage(channel: AudioChannelType, command: AudioCommand): AudioData {
  const { audioType, decodeType } = AUDIO_MAP[channel]
  const HEADER = 12
  const data = Buffer.allocUnsafeSlow(HEADER + 1)
  data.writeUInt32LE(decodeType, 0)
  data.writeFloatLE(0, 4)
  data.writeUInt32LE(audioType, 8)
  data.writeUInt8(command, HEADER)
  const header = new MessageHeader(data.length, MessageType.AudioData)
  return new AudioData(header, data)
}

function buildMediaJsonMessage(media: Record<string, unknown>): MetaData {
  const json = JSON.stringify(media)
  const payload = Buffer.from(json + '\0', 'utf8')
  const data = Buffer.allocUnsafeSlow(4 + payload.length)
  data.writeUInt32LE(MediaType.Data, 0)
  payload.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

function buildAlbumArtMessage(albumArt: Buffer): MetaData {
  const data = Buffer.allocUnsafeSlow(4 + albumArt.length)
  data.writeUInt32LE(MediaType.AlbumCover, 0)
  albumArt.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

function buildNaviJsonMessage(navi: Record<string, unknown>): MetaData {
  const json = JSON.stringify(navi)
  const payload = Buffer.from(json + '\0', 'utf8')
  const data = Buffer.allocUnsafeSlow(4 + payload.length)
  data.writeUInt32LE(NavigationMetaType.DashboardInfo, 0)
  payload.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

function buildNaviImageMessage(image: Buffer): MetaData {
  const data = Buffer.allocUnsafeSlow(4 + image.length)
  data.writeUInt32LE(NavigationMetaType.DashboardImage, 0)
  image.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

function audioLifecycleCommand(channel: AudioChannelType, starting: boolean): AudioCommand {
  switch (channel) {
    case 'media':
      return starting ? AudioCommand.AudioMediaStart : AudioCommand.AudioMediaStop
    case 'speech':
      return starting ? AudioCommand.AudioNaviStart : AudioCommand.AudioNaviStop
    case 'phone':
      return starting ? AudioCommand.AudioOutputStart : AudioCommand.AudioOutputStop
  }
}

export type AaEventBridgeDeps = {
  emitMessage: (msg: Message) => void
  emitCodec: (kind: 'video-codec' | 'cluster-video-codec', codec: VideoCodec) => void
  startMic: (reason: string) => void
  stopMic: (reason: string) => void
  consumeWiredBridge: () => UsbAoapBridge | null
  isClosed: () => boolean
}

export class AaEventBridge {
  private naviBag: Record<string, unknown> = {}
  private naviActive = false
  private naviApp: string | undefined
  private videoFocusEmitted = false
  private clusterFocusEmitted = false

  constructor(
    private readonly aa: AAStack,
    private readonly cfg: AAStackConfig,
    private readonly deps: AaEventBridgeDeps
  ) {}

  wire(): void {
    const { aa, cfg, deps } = this

    aa.on('connected', () => {
      console.log('[AaEventBridge] AAStack connected → DongleReady + Plugged(AndroidAuto)')
      const readyHdr = new MessageHeader(0, MessageType.Open)
      deps.emitMessage(new DongleReady(readyHdr) as Message)
      this.emitPlugged()
    })

    aa.on('disconnected', (reason?: string) => {
      console.log(
        `[AaEventBridge] AAStack disconnected (${reason ?? 'no reason'}) — supervisor stays up for retry`
      )
      this.naviBag = {}
      this.naviActive = false
      this.naviApp = undefined

      if (this.videoFocusEmitted) {
        this.emitCommand(CommandMapping.releaseVideoFocus)
        this.videoFocusEmitted = false
      }

      const hdr = new MessageHeader(0, MessageType.Unplugged)
      deps.emitMessage(new Unplugged(hdr) as Message)

      if (reason === 'pre-RUNNING watchdog') {
        const bridge = deps.consumeWiredBridge()
        if (bridge) {
          console.log('[AaEventBridge] watchdog disconnect — forcing USB re-enumeration')
          void (async () => {
            try {
              await bridge.forceReenum()
            } catch (err) {
              console.warn(`[AaEventBridge] watchdog forceReenum threw: ${(err as Error).message}`)
            }
            try {
              await bridge.stop()
            } catch (err) {
              console.warn(`[AaEventBridge] watchdog bridge stop threw: ${(err as Error).message}`)
            }
          })()
        }
      }
    })

    aa.on('video-focus-projected', () => {
      this.videoFocusEmitted = true
      this.emitCommand(CommandMapping.requestVideoFocus)
    })

    aa.on('cluster-video-focus-projected', () => {
      this.clusterFocusEmitted = true
      this.emitCommand(CommandMapping.requestClusterFocus)
    })

    aa.on('video-frame', (buf: Buffer) => {
      if (!this.videoFocusEmitted) {
        this.videoFocusEmitted = true
        this.emitCommand(CommandMapping.requestVideoFocus)
      }
      const w = cfg.videoWidth ?? 1280
      const h = cfg.videoHeight ?? 720
      deps.emitMessage(buildVideoDataMessage(buf, w, h) as Message)
    })

    aa.on('cluster-video-frame', (buf: Buffer) => {
      if (!this.clusterFocusEmitted) {
        this.clusterFocusEmitted = true
        this.emitCommand(CommandMapping.requestClusterFocus)
      }
      const w = cfg.videoWidth ?? 1280
      const h = cfg.videoHeight ?? 720
      deps.emitMessage(buildVideoDataMessage(buf, w, h, MessageType.ClusterVideoData) as Message)
    })

    aa.on('cluster-video-codec', (codec: VideoCodec) => {
      console.log(`[AaEventBridge] cluster-video-codec=${codec} (phone selection)`)
      deps.emitCodec('cluster-video-codec', codec)
    })

    aa.on('video-codec', (codec: VideoCodec) => {
      console.log(`[AaEventBridge] video-codec=${codec} (phone selection)`)
      deps.emitCodec('video-codec', codec)
    })

    aa.on('audio-frame', (buf: Buffer, _ts: bigint, channel: AudioChannelType) => {
      deps.emitMessage(buildAudioDataMessage(buf, channel) as Message)
    })

    aa.on('audio-start', (channel: AudioChannelType) => {
      const cmd = audioLifecycleCommand(channel, true)
      console.log(`[AaEventBridge] audio-start ${channel} → AudioCommand=${AudioCommand[cmd]}`)
      deps.emitMessage(buildAudioCommandMessage(channel, cmd) as Message)
    })

    aa.on('audio-stop', (channel: AudioChannelType) => {
      const cmd = audioLifecycleCommand(channel, false)
      console.log(`[AaEventBridge] audio-stop ${channel} → AudioCommand=${AudioCommand[cmd]}`)
      deps.emitMessage(buildAudioCommandMessage(channel, cmd) as Message)
    })

    aa.on('mic-start', () => deps.startMic('mic-start'))
    aa.on('mic-stop', () => deps.stopMic('mic-stop'))

    aa.on('voice-session', (active: boolean) => {
      if (active) deps.startMic('voice-session START')
      else deps.stopMic('voice-session END')
    })

    aa.on('host-ui-requested', () => {
      console.log('[AaEventBridge] host-ui-requested → emitting Command(requestHostUI)')
      const buf = Buffer.allocUnsafe(4)
      buf.writeUInt32LE(CommandMapping.requestHostUI, 0)
      const header = new MessageHeader(buf.length, MessageType.Command)
      deps.emitMessage(new Command(header, buf) as Message)
    })

    aa.on('media-metadata', (m: MediaPlaybackMetadata) => {
      const media: Record<string, unknown> = {}
      if (m.song !== undefined) media.MediaSongName = m.song
      if (m.artist !== undefined) media.MediaArtistName = m.artist
      if (m.album !== undefined) media.MediaAlbumName = m.album
      if (m.durationSeconds !== undefined) media.MediaSongDuration = m.durationSeconds * 1000
      if (Object.keys(media).length > 0) {
        deps.emitMessage(buildMediaJsonMessage(media) as Message)
      }
      if (m.albumArt && m.albumArt.length > 0) {
        deps.emitMessage(buildAlbumArtMessage(m.albumArt) as Message)
      }
    })

    aa.on('media-status', (s: MediaPlaybackStatus) => {
      const playStatus = s.state === 'playing' ? 1 : 0
      const media: Record<string, unknown> = { MediaPlayStatus: playStatus }
      if (s.mediaSource !== undefined) media.MediaAPPName = s.mediaSource
      if (s.playbackSeconds !== undefined) media.MediaSongPlayTime = s.playbackSeconds * 1000
      deps.emitMessage(buildMediaJsonMessage(media) as Message)
    })

    aa.on('nav-start', () => {
      this.naviApp = 'Google Maps'
      this.naviActive = true
      this.publishNavi({ NaviStatus: 1, NaviAPPName: this.naviApp })
    })

    aa.on('nav-stop', () => {
      this.naviActive = false
      this.publishNavi({ NaviStatus: 0 })
    })

    aa.on('nav-status', (s: NavigationStatusUpdate) => {
      this.naviActive = s.state === 'active' || s.state === 'rerouting'
      this.publishNavi({ NaviStatus: this.naviActive ? 1 : 0 })
    })

    aa.on('nav-turn', (t: NavigationTurnUpdate) => {
      const patch: Record<string, unknown> = {}
      if (t.road !== undefined) patch.NaviRoadName = t.road
      const maneuver = turnEventToManeuverType(t.event, t.turnSide)
      if (maneuver !== undefined) patch.NaviManeuverType = maneuver
      const side = turnSideToNaviCode(t.turnSide)
      if (side !== undefined) patch.NaviTurnSide = side
      if (t.turnAngle !== undefined) patch.NaviTurnAngle = t.turnAngle
      if (t.turnNumber !== undefined) patch.NaviRoundaboutExitNumber = t.turnNumber
      if (Object.keys(patch).length > 0) this.publishNavi(patch)
      if (t.image && t.image.length > 0) {
        deps.emitMessage(buildNaviImageMessage(t.image) as Message)
      }
    })

    aa.on('nav-distance', (d: NavigationDistanceUpdate) => {
      const patch: Record<string, unknown> = {
        NaviDistanceToDestination: d.distanceMeters,
        NaviTimeToDestination: d.timeToTurnSeconds
      }
      if (d.displayDistanceE3 !== undefined) {
        patch.NaviDisplayDistanceE3 = d.displayDistanceE3
      }
      if (d.displayUnit !== undefined) {
        patch.NaviDisplayDistanceUnit = d.displayUnit
      }
      this.publishNavi(patch)
    })

    aa.on('error', (err: Error) => {
      if (deps.isClosed()) {
        console.debug(`[AaEventBridge] suppressed AAStack error during close: ${err.message}`)
        return
      }
      console.warn(`[AaEventBridge] AAStack transient error: ${err.message}`)
    })
  }

  emitPlugged(): void {
    const buf = Buffer.allocUnsafe(8)
    buf.writeUInt32LE(PhoneType.AndroidAuto, 0)
    buf.writeUInt32LE(1, 4)
    const header = new MessageHeader(buf.length, MessageType.Plugged)
    this.deps.emitMessage(new Plugged(header, buf) as Message)
  }

  private emitCommand(value: CommandMapping): void {
    const buf = Buffer.allocUnsafe(4)
    buf.writeUInt32LE(value, 0)
    const header = new MessageHeader(buf.length, MessageType.Command)
    this.deps.emitMessage(new Command(header, buf) as Message)
  }

  private publishNavi(patch: Record<string, unknown>): void {
    Object.assign(this.naviBag, patch)
    if (this.naviApp !== undefined && this.naviBag.NaviAPPName === undefined) {
      this.naviBag.NaviAPPName = this.naviApp
    }
    this.deps.emitMessage(buildNaviJsonMessage(this.naviBag) as Message)
  }
}
