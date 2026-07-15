import { MessageHeader, MessageType } from '@projection/messages/common'
import {
  buildAlbumArtMessage,
  buildMediaJsonMessage,
  buildNaviImageMessage,
  buildNaviJsonMessage
} from '@projection/messages/metaBuilders'
import {
  AudioData,
  Command,
  DongleReady,
  type Message,
  NavigationMetaType,
  PhoneType,
  Plugged,
  Unplugged,
  VideoData
} from '@projection/messages/readable'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import {
  navManeuverTypeToCode,
  navManeuverTypeToSide,
  turnEventToManeuverType,
  turnSideToNaviCode
} from './stack/channels/navManeuverMap'
import {
  type AAStack,
  type AAStackConfig,
  type AudioChannelType,
  type MediaPlaybackMetadata,
  type MediaPlaybackStatus,
  type NavigationDistanceUpdate,
  type NavigationPositionUpdate,
  type NavigationStateUpdate,
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

export function buildVideoDataMessage(
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
  emitDevicePresence: (d: { name: string; model: string; instanceId: string; ip: string }) => void
  emitDeviceStatus: (s: Record<string, unknown>) => void
  emitConnected?: () => void
  emitDisconnected?: (reason?: string) => void
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
      console.log('[AaEventBridge] AAStack connected')
      deps.emitConnected?.()
    })

    aa.on('disconnected', (reason?: string) => {
      console.log(
        `[AaEventBridge] AAStack disconnected (${reason ?? 'no reason'}) — supervisor stays up for retry`
      )
      deps.emitDisconnected?.(reason)
      this.naviBag = {}
      this.naviActive = false
      this.naviApp = undefined

      if (this.videoFocusEmitted) {
        this.emitCommand(CommandMapping.releaseVideoFocus)
        this.videoFocusEmitted = false
      }

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

    aa.on('device-info', (d: { name: string; model: string; instanceId: string; ip: string }) => {
      deps.emitDevicePresence(d)
    })

    aa.on('device-status', (s: Record<string, unknown>) => {
      deps.emitDeviceStatus(s)
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
        NaviRemainDistance: d.distanceMeters
      }
      if (d.displayDistanceE3 !== undefined) {
        patch.NaviDisplayDistanceE3 = d.displayDistanceE3
      }
      if (d.displayUnit !== undefined) {
        patch.NaviDisplayDistanceUnit = d.displayUnit
      }
      this.publishNavi(patch)
    })

    // Modern nav (AA ≥ 1.7): current step maneuver/road + destination address.
    aa.on('nav-state', (s: NavigationStateUpdate) => {
      const patch: Record<string, unknown> = {}
      if (s.maneuverType !== undefined) {
        const code = navManeuverTypeToCode(s.maneuverType)
        if (code !== undefined) patch.NaviManeuverType = code
        const side = navManeuverTypeToSide(s.maneuverType)
        if (side !== undefined) patch.NaviTurnSide = side
      }
      if (s.roadName) patch.NaviRoadName = s.roadName
      if (s.destinationAddress) patch.NaviDestinationName = s.destinationAddress
      if (Object.keys(patch).length > 0) this.publishNavi(patch)
    })

    // Modern nav (AA ≥ 1.7): live distance/time to the next step AND to the destination.
    aa.on('nav-position', (p: NavigationPositionUpdate) => {
      const patch: Record<string, unknown> = {}
      // step_distance = distance to the next maneuver (shown next to the turn arrow)
      if (p.stepDistanceMeters !== undefined) patch.NaviRemainDistance = p.stepDistanceMeters
      // destination distance + remaining time + arrival clock — the real trip figures
      if (p.destinationMeters !== undefined) patch.NaviDistanceToDestination = p.destinationMeters
      if (p.timeToArrivalSeconds !== undefined) patch.NaviTimeToDestination = p.timeToArrivalSeconds
      if (p.etaText) patch.NaviETA = p.etaText
      if (Object.keys(patch).length > 0) this.publishNavi(patch)
    })

    aa.on('error', (err: Error) => {
      if (deps.isClosed()) {
        console.debug(`[AaEventBridge] suppressed AAStack error during close: ${err.message}`)
        return
      }
      console.warn(`[AaEventBridge] AAStack transient error: ${err.message}`)
    })
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
