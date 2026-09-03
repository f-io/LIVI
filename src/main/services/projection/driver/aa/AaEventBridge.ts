import {
  AudioData,
  Command,
  DuckAudio,
  MediaData,
  type MediaInfo,
  MediaType,
  type Message,
  NavigationData,
  NavigationMetaType,
  type NaviInfo
} from '@projection/messages/readable'
import { AudioCommand, CommandMapping } from '@shared/types/ProjectionEnums'
import {
  navManeuverTypeToCode,
  navManeuverTypeToSide,
  turnEventToManeuverType,
  turnSideToNaviCode
} from './stack/channels/navManeuverMap'
import { CH } from './stack/constants'
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

// audioType uses the CarPlay numbering ProjectionAudio keys its streams by, not the AA wire types.
// System sounds take the nav type like CarPlay alerts, calls never reach this link (HFP).
const AUDIO_MAP: Record<AudioChannelType, { audioType: number; decodeType: number }> = {
  media: { audioType: 3, decodeType: 4 },
  speech: { audioType: 4, decodeType: 5 },
  system: { audioType: 4, decodeType: 5 }
}

const AUDIO_CHANNEL_ID: Record<AudioChannelType, number> = {
  media: CH.MEDIA_AUDIO,
  speech: CH.SPEECH_AUDIO,
  system: CH.SYSTEM_AUDIO
}

/** What a helper-carried session needs to route its media straight into the host. */
export type AaMediaSinkDeps = {
  /** The host's feed socket path, '' when the host cannot provide one. */
  feedPath: () => Promise<string>
  videoPlaneId: (cluster: boolean) => number
  /** Creates the plane so the fed frames find a decoder. */
  primeVideo: (cluster: boolean) => void
  /** The helper saw the first frame of a stream, geometry and focus follow. */
  noteVideoStarted: (cluster: boolean, width: number, height: number) => void
  /** The host's driver-fed streams, tagged with the channel they were opened for. */
  audioOutputs: () => Array<{ audioType: number; streamId: number; tag?: string }>
  onAudioOutput: (cb: (audioType: number, streamId: number, tag?: string) => void) => () => void
  /** Opens the host stream for a channel's format, samples never pass through here. */
  primeAudio: (audioType: number, sampleRate: number, channels: number, tag: string) => void
  /** Sets the level of the host-fed streams of this audioType. */
  setHostVolume: (audioType: number, level: number, rampMs: number) => void
}

function buildAudioCommandMessage(channel: AudioChannelType, command: AudioCommand): AudioData {
  const { audioType, decodeType } = AUDIO_MAP[channel]
  return new AudioData({ decodeType, audioType, command })
}

function audioLifecycleCommand(channel: AudioChannelType, starting: boolean): AudioCommand {
  switch (channel) {
    case 'media':
      return starting ? AudioCommand.AudioMediaStart : AudioCommand.AudioMediaStop
    case 'speech':
    case 'system':
      return starting ? AudioCommand.AudioNaviStart : AudioCommand.AudioNaviStop
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
  isClosed: () => boolean
  mediaSink?: AaMediaSinkDeps
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
        `[AaEventBridge] AAStack disconnected (${reason ?? 'no reason'}), supervisor stays up for retry`
      )
      deps.emitDisconnected?.(reason)
      this.naviBag = {}
      this.naviActive = false
      this.naviApp = undefined

      if (this.videoFocusEmitted) {
        this.emitCommand(CommandMapping.releaseVideoFocus)
        this.videoFocusEmitted = false
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

    aa.on('cluster-video-codec', (codec: VideoCodec) => {
      console.log(`[AaEventBridge] cluster-video-codec=${codec} (phone selection)`)
      deps.emitCodec('cluster-video-codec', codec)
      this.pushVideoSink(true, codec)
    })

    aa.on('video-codec', (codec: VideoCodec) => {
      console.log(`[AaEventBridge] video-codec=${codec} (phone selection)`)
      deps.emitCodec('video-codec', codec)
      this.pushVideoSink(false, codec)
    })

    // Helper-carried sessions: the frames go to the host directly, only their
    // arrival is reported here.
    aa.on('video-started', () => {
      if (!this.videoFocusEmitted) {
        this.videoFocusEmitted = true
        this.emitCommand(CommandMapping.requestVideoFocus)
      }
      deps.mediaSink?.noteVideoStarted(false, cfg.videoWidth ?? 1280, cfg.videoHeight ?? 720)
    })

    aa.on('cluster-video-started', () => {
      if (!this.clusterFocusEmitted) {
        this.clusterFocusEmitted = true
        this.emitCommand(CommandMapping.requestClusterFocus)
      }
      deps.mediaSink?.noteVideoStarted(true, cfg.videoWidth ?? 1280, cfg.videoHeight ?? 720)
    })

    if (deps.mediaSink) {
      const sink = deps.mediaSink
      const off = sink.onAudioOutput((_audioType, streamId, tag) =>
        this.pushAudioSink(streamId, tag)
      )
      aa.on('connected', () => {
        for (const o of sink.audioOutputs()) this.pushAudioSink(o.streamId, o.tag)
      })
      aa.on('disconnected', off)
      // One host stream per channel, so speech and system never share a decoder.
      aa.on('audio-setup', (channel: AudioChannelType, sampleRate: number, channels: number) => {
        sink.primeAudio(AUDIO_MAP[channel].audioType, sampleRate, channels, channel)
      })
    }

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

    aa.on('audio-focus', (focusType: number) => {
      const msg = focusType === 3 ? new DuckAudio(0.2, 500) : new DuckAudio(1, 1500)
      console.log(`[AaEventBridge] audio-focus type=${focusType} → duck level=${msg.level}`)
      deps.emitMessage(msg as Message)
    })

    aa.on('mic-start', () => deps.startMic('mic-start'))
    aa.on('mic-stop', () => deps.stopMic('mic-stop'))

    aa.on('voice-session', (active: boolean) => {
      if (active) deps.startMic('voice-session START')
      else deps.stopMic('voice-session END')
    })

    aa.on('host-ui-requested', () => {
      console.log('[AaEventBridge] host-ui-requested → emitting Command(requestHostUI)')
      deps.emitMessage(new Command(CommandMapping.requestHostUI))
    })

    aa.on('media-metadata', (m: MediaPlaybackMetadata) => {
      const media: Record<string, unknown> = {}
      if (m.song !== undefined) media.MediaSongName = m.song
      if (m.artist !== undefined) media.MediaArtistName = m.artist
      if (m.album !== undefined) media.MediaAlbumName = m.album
      if (m.durationSeconds !== undefined) media.MediaSongDuration = m.durationSeconds * 1000
      if (Object.keys(media).length > 0) {
        deps.emitMessage(
          new MediaData(MediaType.Data, { type: MediaType.Data, media: media as MediaInfo })
        )
      }
      if (m.albumArt && m.albumArt.length > 0) {
        deps.emitMessage(
          new MediaData(MediaType.AlbumCoverAlt, {
            type: MediaType.AlbumCoverAlt,
            base64Image: m.albumArt.toString('base64')
          })
        )
      }
    })

    aa.on('media-status', (s: MediaPlaybackStatus) => {
      const playStatus = s.state === 'playing' ? 1 : 0
      const media: Record<string, unknown> = { MediaPlayStatus: playStatus }
      if (s.mediaSource !== undefined) media.MediaAPPName = s.mediaSource
      if (s.playbackSeconds !== undefined) media.MediaSongPlayTime = s.playbackSeconds * 1000
      deps.emitMessage(
        new MediaData(MediaType.Data, { type: MediaType.Data, media: media as MediaInfo })
      )
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
        deps.emitMessage(
          new NavigationData(NavigationMetaType.DashboardImage, {
            NaviImageBase64: t.image.toString('base64')
          })
        )
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
      // destination distance + remaining time + arrival clock, the real trip figures
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

  // The plane is primed first so the host has a decoder when the feed starts.
  private pushVideoSink(cluster: boolean, codec: VideoCodec): void {
    const sink = this.deps.mediaSink
    if (!sink) return
    sink.primeVideo(cluster)
    const entry = {
      ch: cluster ? CH.CLUSTER_VIDEO : CH.VIDEO,
      id: sink.videoPlaneId(cluster),
      codec
    }
    void sink.feedPath().then((feed) => this.deliverVideoSink(feed, entry))
  }

  private deliverVideoSink(feed: string, entry: unknown): void {
    if (this.deps.isClosed()) return
    if (!feed) console.warn('[AaEventBridge] host has no media feed, video will not show')
    this.aa.sendMediaSink({ feed, video: [entry] })
  }

  // Routes a host stream to the one channel it was opened for.
  private pushAudioSink(streamId: number, tag: string | undefined): void {
    const sink = this.deps.mediaSink
    if (!sink) return
    if (!tag || !(tag in AUDIO_CHANNEL_ID)) return
    const ch = AUDIO_CHANNEL_ID[tag as AudioChannelType]
    void sink.feedPath().then((feed) => this.deliverAudioSink(feed, ch, streamId))
  }

  private deliverAudioSink(feed: string, ch: number, streamId: number): void {
    if (this.deps.isClosed()) return
    this.aa.sendMediaSink({ feed, audio: [{ ch, id: streamId }] })
  }

  private emitCommand(value: CommandMapping): void {
    this.deps.emitMessage(new Command(value))
  }

  private publishNavi(patch: Record<string, unknown>): void {
    Object.assign(this.naviBag, patch)
    if (this.naviApp !== undefined && this.naviBag.NaviAPPName === undefined) {
      this.naviBag.NaviAPPName = this.naviApp
    }
    this.deps.emitMessage(
      new NavigationData(NavigationMetaType.DashboardInfo, { ...this.naviBag } as NaviInfo)
    )
  }
}
