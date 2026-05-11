/**
 * aaDriver — IPhoneDriver for native wireless Android Auto.
 * Owns AaBluetoothSupervisor (BT/Wi-Fi pairing) + AAStack (TCP 5277 protocol).
 * Translates AAStack events to LIVI domain messages.
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import { DEBUG } from '@main/constants'
import { Microphone } from '@main/services/audio'
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
import {
  type SendableMessage,
  SendCloseDongle,
  SendCommand,
  SendDisconnectPhone,
  SendMultiTouch,
  SendTouch
} from '@projection/messages/sendable'
import type { DongleConfig } from '@shared/types'
import { CarType } from '@shared/types/DongleConfig'
import {
  AudioCommand,
  CommandMapping,
  MultiTouchAction,
  TouchAction
} from '@shared/types/ProjectionEnums'
import {
  computeAndroidAutoDpi,
  isClusterDisplayed,
  matchFittingAAResolution,
  pixelAspectRatioE4
} from '@shared/utils'
import type { Device } from 'usb'
import type { IPhoneDriver } from '../IPhoneDriver'
import { AaBluetoothSupervisor } from './aaBluetoothSupervisor'
import { AOAP_LOOPBACK_HOST, AOAP_LOOPBACK_PORT } from './stack/aoap/constants'
import { turnEventToManeuverType, turnSideToNaviCode } from './stack/channels/navManeuverMap'
import {
  AAStack,
  type AAStackConfig,
  type AudioChannelType,
  BUTTON_KEY,
  type MediaPlaybackMetadata,
  type MediaPlaybackStatus,
  type NavigationDistanceUpdate,
  type NavigationStatusUpdate,
  type NavigationTurnUpdate,
  TOUCH_ACTION,
  type TouchPointer,
  type VideoCodec
} from './stack/index'
import { UsbAoapBridge } from './stack/transport/UsbAoapBridge'

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
  data.writeUInt32LE(0, 8) // flags
  data.writeUInt32LE(buf.length, 12)
  data.writeUInt32LE(0, 16) // unknown
  buf.copy(data, HEADER)
  const header = new MessageHeader(data.length, type)
  return new VideoData(header, data)
}

const AUDIO_MAP: Record<AudioChannelType, { audioType: number; decodeType: number }> = {
  media: { audioType: 3, decodeType: 4 },
  speech: { audioType: 1, decodeType: 5 },
  phone: { audioType: 2, decodeType: 5 }
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

// Map an AA audio-channel start/stop transition to the corresponding LIVI
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

/**
 * Map a single-pointer TouchAction to PointerAction enum
 */
function mapTouchAction(action: TouchAction): number {
  switch (action) {
    case TouchAction.Down:
      return TOUCH_ACTION.DOWN
    case TouchAction.Move:
      return TOUCH_ACTION.MOVED
    case TouchAction.Up:
      return TOUCH_ACTION.UP
  }
  return TOUCH_ACTION.MOVED
}

/** Map LIVI's CarType to aap_protobuf FuelType[] for the AA SDR. */
function mapCarTypeToFuelTypes(carType: CarType | undefined): number[] {
  switch (carType) {
    case CarType.HybridGasoline:
      return [CarType.Gasoline, CarType.Electric]
    case CarType.HybridDiesel:
      return [CarType.Diesel, CarType.Electric]
    case undefined:
    case CarType.Unknown:
      return [CarType.Gasoline]
    default:
      return [carType]
  }
}

export interface AaDriverOptions {
  supervisor?: AaBluetoothSupervisor | null
  onWillReenumerate?: (durationMs: number) => void
}

export class AaDriver extends EventEmitter implements IPhoneDriver {
  private _aa: AAStack | null = null
  private _supervisor: AaBluetoothSupervisor | null
  private _started = false
  private _closed = false
  private _touchW = 1280
  private _touchH = 720
  private _touchInsetLeft = 0
  private _touchInsetRight = 0
  private _touchInsetTop = 0
  private _touchInsetBottom = 0
  private _mic: Microphone | null = null
  private _micActive = false

  // Accumulating NaviBag — flushed via buildNaviJsonMessage on every patch.
  private _naviBag: Record<string, unknown> = {}
  private _naviActive = false
  private _naviApp: string | undefined
  private _videoFocusEmitted = false
  private _clusterFocusEmitted = false
  private _hevcSupported = false
  private _vp9Supported = false
  private _av1Supported = false
  private _initialNightMode: boolean | undefined = undefined
  private _aaCfg: AAStackConfig | null = null
  private _wiredDevice: Device | null = null
  private _wiredBridge: UsbAoapBridge | null = null
  private _wiredClientSocket: net.Socket | null = null
  private readonly _onWillReenumerate: ((durationMs: number) => void) | undefined

  constructor(opts: AaDriverOptions = {}) {
    super()
    this._supervisor = opts.supervisor ?? new AaBluetoothSupervisor({ maxRestarts: 5 })
    this._onWillReenumerate = opts.onWillReenumerate
  }

  setHevcSupported(supported: boolean): void {
    this._hevcSupported = supported
    if (this._aaCfg) this._aaCfg.hevcSupported = supported
  }

  setVp9Supported(supported: boolean): void {
    this._vp9Supported = supported
    if (this._aaCfg) this._aaCfg.vp9Supported = supported
  }

  setAv1Supported(supported: boolean): void {
    this._av1Supported = supported
    if (this._aaCfg) this._aaCfg.av1Supported = supported
  }

  setInitialNightMode(value: boolean | undefined): void {
    this._initialNightMode = value
    if (this._aaCfg) this._aaCfg.initialNightMode = value
  }

  setWiredDevice(device: Device | null): void {
    this._wiredDevice = device
  }

  isWiredMode(): boolean {
    return this._wiredDevice !== null
  }

  // Soft restart of the AAStack TCP listener
  async restartStack(): Promise<void> {
    if (!this._aa) {
      console.log('[aaDriver] restartStack: no AAStack to restart')
      return
    }
    console.log('[aaDriver] restartStack — phone will reconnect TCP shortly')
    try {
      this._aa.stop()
    } catch (err) {
      console.warn(`[aaDriver] AAStack.stop on restartStack threw: ${(err as Error).message}`)
    }
    await new Promise((r) => setTimeout(r, 200))
    try {
      this._aa.start()
    } catch (err) {
      console.warn(`[aaDriver] AAStack.start on restartStack threw: ${(err as Error).message}`)
    }
  }

  async start(cfg: DongleConfig): Promise<boolean> {
    if (this._started) return true
    this._started = true
    this._closed = false

    const wired = this._wiredDevice !== null

    // 1. Bring up python BT/Wi-Fi stack first — only for the wireless path.
    if (this._supervisor && !wired) {
      this._supervisor.on('stdout', (line) => console.log(`[aa-bt] ${line}`))
      this._supervisor.on('stderr', (line) => console.warn(`[aa-bt!] ${line}`))
      this._supervisor.on('error', (err) => {
        console.warn(`[aaDriver] supervisor error: ${err.message}`)
      })
      this._supervisor.start(cfg)
    }

    // 2. AAStackConfig
    const h264Only = !(this._hevcSupported || this._vp9Supported || this._av1Supported)
    const aaFit = matchFittingAAResolution({ width: cfg.width, height: cfg.height }, { h264Only })
    const tierW = aaFit.width
    const tierH = aaFit.height
    const aaDpi = cfg.dpi > 0 ? cfg.dpi : computeAndroidAutoDpi(tierW, tierH)
    const clusterFit = matchFittingAAResolution(
      { width: cfg.clusterWidth, height: cfg.clusterHeight },
      { h264Only }
    )
    const clusterTierW = clusterFit.width
    const clusterTierH = clusterFit.height
    const resolvedClusterDpi =
      cfg.clusterDpi > 0 ? cfg.clusterDpi : computeAndroidAutoDpi(clusterTierW, clusterTierH)
    const name = cfg.carName?.trim() ? cfg.carName : 'LIVI'
    const aaCfg: AAStackConfig = {
      huName: name,
      videoWidth: tierW,
      videoHeight: tierH,
      videoDpi: aaDpi,
      videoFps: cfg.fps === 60 ? 60 : 30,
      pixelAspectRatioE4: pixelAspectRatioE4(
        { width: cfg.width, height: cfg.height },
        { width: tierW, height: tierH }
      ),
      displayWidth: cfg.width,
      displayHeight: cfg.height,
      mainSafeAreaTop: cfg.projectionSafeAreaTop,
      mainSafeAreaBottom: cfg.projectionSafeAreaBottom,
      mainSafeAreaLeft: cfg.projectionSafeAreaLeft,
      mainSafeAreaRight: cfg.projectionSafeAreaRight,
      driverPosition: cfg.hand === 1 ? 1 : 0,
      wifiSsid: name,
      wifiPassword: cfg.wifiPassword || '12345678',
      wifiChannel: cfg.wifiChannel,
      fuelTypes: mapCarTypeToFuelTypes(cfg.carType),
      evConnectorTypes: cfg.evConnectorTypes,
      hevcSupported: this._hevcSupported,
      vp9Supported: this._vp9Supported,
      av1Supported: this._av1Supported,
      initialNightMode: this._initialNightMode,
      clusterEnabled: isClusterDisplayed(cfg),
      clusterWidth: cfg.clusterWidth,
      clusterHeight: cfg.clusterHeight,
      clusterTierWidth: clusterTierW,
      clusterTierHeight: clusterTierH,
      clusterPixelAspectRatioE4: pixelAspectRatioE4(
        { width: cfg.clusterWidth, height: cfg.clusterHeight },
        { width: clusterTierW, height: clusterTierH }
      ),
      clusterFps: cfg.clusterFps,
      clusterDpi: resolvedClusterDpi,
      clusterSafeAreaTop: cfg.clusterSafeAreaTop,
      clusterSafeAreaBottom: cfg.clusterSafeAreaBottom,
      clusterSafeAreaLeft: cfg.clusterSafeAreaLeft,
      clusterSafeAreaRight: cfg.clusterSafeAreaRight,
      disableAudioOutput: Boolean(cfg.disableAudioOutput)
    }
    const displayAR = cfg.width / cfg.height
    const tierAR = tierW / tierH
    console.log(
      `[aaDriver] display ${cfg.width}×${cfg.height} (AR ${displayAR.toFixed(3)}) → ` +
        `AA tier ${tierW}×${tierH} (AR ${tierAR.toFixed(3)}) @${aaDpi}dpi, ` +
        `PAR e4=${aaCfg.pixelAspectRatioE4}`
    )
    const clusterActive = isClusterDisplayed(cfg)
    console.log(
      `[aaDriver] clusterDisplayed=${clusterActive} ` +
        `(cluster main=${cfg.cluster?.main ?? false} dash=${cfg.cluster?.dash ?? false} ` +
        `aux=${cfg.cluster?.aux ?? false}; channel will ${clusterActive ? 'be advertised' : 'NOT be advertised'} in SDR)`
    )
    if (clusterActive) {
      const cAR = cfg.clusterWidth / cfg.clusterHeight
      const cTierAR = clusterTierW / clusterTierH
      console.log(
        `[aaDriver] cluster ${cfg.clusterWidth}×${cfg.clusterHeight} (AR ${cAR.toFixed(3)}) → ` +
          `AA tier ${clusterTierW}×${clusterTierH} (AR ${cTierAR.toFixed(3)}) @${resolvedClusterDpi}dpi, ` +
          `PAR e4=${aaCfg.clusterPixelAspectRatioE4}`
      )
    }

    let arWMargin = 0
    let arHMargin = 0
    if (cfg.width > 0 && cfg.height > 0 && tierW > 0 && tierH > 0) {
      if (displayAR > tierAR) {
        const contentH = Math.round(tierW / displayAR) & ~1
        arHMargin = Math.max(0, tierH - contentH)
      } else if (displayAR < tierAR) {
        const contentW = Math.round(tierH * displayAR) & ~1
        arWMargin = Math.max(0, tierW - contentW)
      }
    }
    const arTop = Math.floor(arHMargin / 2)
    const arBottom = arHMargin - arTop
    const arLeft = Math.floor(arWMargin / 2)
    const arRight = arWMargin - arLeft

    this._touchW = tierW
    this._touchH = tierH
    this._touchInsetTop = arTop + Math.max(0, cfg.projectionSafeAreaTop ?? 0)
    this._touchInsetBottom = arBottom + Math.max(0, cfg.projectionSafeAreaBottom ?? 0)
    this._touchInsetLeft = arLeft + Math.max(0, cfg.projectionSafeAreaLeft ?? 0)
    this._touchInsetRight = arRight + Math.max(0, cfg.projectionSafeAreaRight ?? 0)

    this._aaCfg = aaCfg
    const aa = new AAStack(aaCfg)
    this._aa = aa

    aa.on('connected', () => {
      console.log('[aaDriver] AAStack connected → DongleReady + Plugged(AndroidAuto)')
      const readyHdr = new MessageHeader(0, MessageType.Open)
      this.emit('message', new DongleReady(readyHdr) as Message)
      this._emitPlugged()
    })

    aa.on('disconnected', (reason?: string) => {
      console.log(
        `[aaDriver] AAStack disconnected (${reason ?? 'no reason'}) — supervisor stays up for retry`
      )
      // Drop any accumulated nav state so the next session starts fresh
      this._naviBag = {}
      this._naviActive = false
      this._naviApp = undefined

      if (this._videoFocusEmitted) {
        this._emitCommand(CommandMapping.releaseVideoFocus)
        this._videoFocusEmitted = false
      }

      const hdr = new MessageHeader(0, MessageType.Unplugged)
      this.emit('message', new Unplugged(hdr) as Message)

      // Watchdog recovery: tear down the bridge with a USB re-enum kick
      if (reason === 'pre-RUNNING watchdog' && this._wiredBridge) {
        const bridge = this._wiredBridge
        this._wiredBridge = null
        console.log('[aaDriver] watchdog disconnect — forcing USB re-enumeration')
        void (async () => {
          try {
            await bridge.forceReenum()
          } catch (err) {
            console.warn(`[aaDriver] watchdog forceReenum threw: ${(err as Error).message}`)
          }
          try {
            await bridge.stop()
          } catch (err) {
            console.warn(`[aaDriver] watchdog bridge stop threw: ${(err as Error).message}`)
          }
        })()
      }
    })

    // VideoFocusRequest(PROJECTED)
    aa.on('video-focus-projected', () => {
      this._videoFocusEmitted = true
      this._emitCommand(CommandMapping.requestVideoFocus)
    })

    aa.on('cluster-video-focus-projected', () => {
      this._clusterFocusEmitted = true
      this._emitCommand(CommandMapping.requestClusterFocus)
    })

    aa.on('video-frame', (buf: Buffer, _ts: bigint) => {
      if (!this._videoFocusEmitted) {
        this._videoFocusEmitted = true
        this._emitCommand(CommandMapping.requestVideoFocus)
      }
      const w = aaCfg.videoWidth ?? 1280
      const h = aaCfg.videoHeight ?? 720
      this.emit('message', buildVideoDataMessage(buf, w, h) as Message)
    })

    aa.on('cluster-video-frame', (buf: Buffer, _ts: bigint) => {
      if (!this._clusterFocusEmitted) {
        this._clusterFocusEmitted = true
        this._emitCommand(CommandMapping.requestClusterFocus)
      }
      const w = aaCfg.videoWidth ?? 1280
      const h = aaCfg.videoHeight ?? 720
      this.emit(
        'message',
        buildVideoDataMessage(buf, w, h, MessageType.ClusterVideoData) as Message
      )
    })

    aa.on('cluster-video-codec', (codec: VideoCodec) => {
      console.log(`[aaDriver] cluster-video-codec=${codec} (phone selection)`)
      this.emit('cluster-video-codec', codec)
    })

    aa.on('video-codec', (codec: VideoCodec) => {
      console.log(`[aaDriver] video-codec=${codec} (phone selection)`)
      this.emit('video-codec', codec)
    })

    aa.on(
      'audio-frame',
      (buf: Buffer, _ts: bigint, channel: AudioChannelType, _channelId: number) => {
        this.emit('message', buildAudioDataMessage(buf, channel) as Message)
      }
    )

    aa.on('audio-start', (channel: AudioChannelType, _channelId: number) => {
      const cmd = audioLifecycleCommand(channel, true)
      console.log(`[aaDriver] audio-start ${channel} → AudioCommand=${AudioCommand[cmd]}`)
      this.emit('message', buildAudioCommandMessage(channel, cmd) as Message)
    })

    aa.on('audio-stop', (channel: AudioChannelType, _channelId: number) => {
      const cmd = audioLifecycleCommand(channel, false)
      console.log(`[aaDriver] audio-stop ${channel} → AudioCommand=${AudioCommand[cmd]}`)
      this.emit('message', buildAudioCommandMessage(channel, cmd) as Message)
    })

    // Mic lifecycle: phone opens / closes its mic-input channel.
    aa.on('mic-start', () => this._startMicCapture('mic-start'))
    aa.on('mic-stop', () => this._stopMicCapture('mic-stop'))

    // Voice-assist trigger: phone signals it expects mic input
    aa.on('voice-session', (active: boolean) => {
      if (active) this._startMicCapture('voice-session START')
      else this._stopMicCapture('voice-session END')
    })

    // Phone asked HU to swap to its native (host) UI.
    aa.on('host-ui-requested', () => {
      console.log('[aaDriver] host-ui-requested → emitting Command(requestHostUI)')
      const buf = Buffer.allocUnsafe(4)
      buf.writeUInt32LE(CommandMapping.requestHostUI, 0)
      const header = new MessageHeader(buf.length, MessageType.Command)
      this.emit('message', new Command(header, buf) as Message)
    })

    aa.on('media-metadata', (m: MediaPlaybackMetadata) => {
      // MediaPlaybackMetadata (track info) → LIVI MediaData JSON keys.
      const media: Record<string, unknown> = {}
      if (m.song !== undefined) media.MediaSongName = m.song
      if (m.artist !== undefined) media.MediaArtistName = m.artist
      if (m.album !== undefined) media.MediaAlbumName = m.album
      if (m.durationSeconds !== undefined) media.MediaSongDuration = m.durationSeconds * 1000
      if (Object.keys(media).length > 0) {
        this.emit('message', buildMediaJsonMessage(media) as Message)
      }
      if (m.albumArt && m.albumArt.length > 0) {
        this.emit('message', buildAlbumArtMessage(m.albumArt) as Message)
      }
    })

    aa.on('media-status', (s: MediaPlaybackStatus) => {
      // MediaPlaybackStatus (per-tick playback state)
      const playStatus = s.state === 'playing' ? 1 : 0
      const media: Record<string, unknown> = { MediaPlayStatus: playStatus }
      if (s.mediaSource !== undefined) media.MediaAPPName = s.mediaSource
      // ms — see comment in media-metadata listener.
      if (s.playbackSeconds !== undefined) media.MediaSongPlayTime = s.playbackSeconds * 1000
      this.emit('message', buildMediaJsonMessage(media) as Message)
    })

    aa.on('nav-start', () => {
      this._naviApp = 'Google Maps'
      this._naviActive = true
      this._publishNavi({ NaviStatus: 1, NaviAPPName: this._naviApp })
    })

    aa.on('nav-stop', () => {
      this._naviActive = false
      this._publishNavi({ NaviStatus: 0 })
    })

    aa.on('nav-status', (s: NavigationStatusUpdate) => {
      this._naviActive = s.state === 'active' || s.state === 'rerouting'
      this._publishNavi({ NaviStatus: this._naviActive ? 1 : 0 })
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
      if (Object.keys(patch).length > 0) this._publishNavi(patch)
      // Turn icon bitmap — forwarded verbatim as DashboardImage.
      if (t.image && t.image.length > 0) {
        this.emit('message', buildNaviImageMessage(t.image) as Message)
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
      this._publishNavi(patch)
    })

    aa.on('error', (err: Error) => {
      // Suppress error spam while we're tearing down
      if (this._closed) {
        console.debug(`[aaDriver] suppressed AAStack error during close: ${err.message}`)
        return
      }
      console.warn(`[aaDriver] AAStack transient error: ${err.message}`)
    })

    if (wired) {
      const ok = await this._startWiredBridge(aa)
      if (!ok) {
        this._started = false
        return false
      }
      return true
    }

    aa.start()
    console.log('[aaDriver] AA stack listening on TCP 5277')
    return true
  }

  private async _startWiredBridge(aa: AAStack): Promise<boolean> {
    const device = this._wiredDevice
    if (!device) return false

    const bridge = new UsbAoapBridge(device, this._onWillReenumerate)
    this._wiredBridge = bridge

    bridge.on('error', (err: Error) => {
      if (this._closed) return
      console.warn(`[aaDriver] wired bridge error: ${err.message}`)
    })
    bridge.on('closed', () => {
      console.log('[aaDriver] wired bridge closed')
    })
    bridge.once('ready', ({ host, port }: { host: string; port: number }) => {
      if (this._closed) return
      console.log(`[aaDriver] wired bridge ready on ${host}:${port}, dialling AAStack`)
      const sock = net.createConnection({ host, port, allowHalfOpen: true })
      this._wiredClientSocket = sock
      sock.once('connect', () => {
        if (this._closed) {
          try {
            sock.destroy()
          } catch {
            /* ignore */
          }
          return
        }
        console.log('[aaDriver] wired loopback connected → AAStack.attachSocket')
        try {
          aa.attachSocket(sock)
        } catch (err) {
          console.error(`[aaDriver] AAStack.attachSocket threw: ${(err as Error).message}`)
        }
      })
      sock.on('error', (err: Error) => {
        console.warn(`[aaDriver] wired loopback socket error: ${err.message}`)
      })
    })

    try {
      await bridge.start(AOAP_LOOPBACK_PORT)
      console.log('[aaDriver] wired AA bridge started on loopback')
      return true
    } catch (err) {
      console.error(`[aaDriver] wired bridge start failed: ${(err as Error).message}`)
      try {
        await bridge.stop()
      } catch {
        /* ignore */
      }
      this._wiredBridge = null
      return false
    }
  }

  private _publishNavi(patch: Record<string, unknown>): void {
    Object.assign(this._naviBag, patch)
    if (this._naviApp !== undefined && this._naviBag.NaviAPPName === undefined) {
      this._naviBag.NaviAPPName = this._naviApp
    }
    this.emit('message', buildNaviJsonMessage(this._naviBag) as Message)
  }

  private _emitPlugged(): void {
    const pluggedBuf = Buffer.allocUnsafe(8)
    pluggedBuf.writeUInt32LE(PhoneType.AndroidAuto, 0)
    pluggedBuf.writeUInt32LE(1, 4) // wifi available
    const pluggedHdr = new MessageHeader(pluggedBuf.length, MessageType.Plugged)
    this.emit('message', new Plugged(pluggedHdr, pluggedBuf) as Message)
  }

  private _emitCommand(value: CommandMapping): void {
    const buf = Buffer.allocUnsafe(4)
    buf.writeUInt32LE(value, 0)
    const header = new MessageHeader(buf.length, MessageType.Command)
    this.emit('message', new Command(header, buf) as Message)
  }

  // ── Vehicle-data push API ──────────────────────────────────────────────────
  // No-op when no active session. Caller does unit conversion + rate-limiting.

  sendFuelData(level: number, range?: number, lowFuelWarning?: boolean): void {
    this._aa?.sendFuelData(level, range, lowFuelWarning)
  }
  sendSpeedData(speedMmS: number, cruiseEngaged?: boolean, cruiseSetSpeedMmS?: number): void {
    this._aa?.sendSpeedData(speedMmS, cruiseEngaged, cruiseSetSpeedMmS)
  }
  sendRpmData(rpmE3: number): void {
    this._aa?.sendRpmData(rpmE3)
  }
  sendGearData(gear: number): void {
    this._aa?.sendGearData(gear)
  }
  sendNightModeData(nightMode: boolean): void {
    this._aa?.sendNightModeData(nightMode)
  }
  sendParkingBrakeData(engaged: boolean): void {
    this._aa?.sendParkingBrakeData(engaged)
  }
  sendLightData(headLight?: 1 | 2 | 3, hazardLights?: boolean, turnIndicator?: 1 | 2 | 3): void {
    this._aa?.sendLightData(headLight, hazardLights, turnIndicator)
  }
  sendEnvironmentData(temperatureE3?: number, pressureE3?: number, rain?: number): void {
    this._aa?.sendEnvironmentData(temperatureE3, pressureE3, rain)
  }
  sendOdometerData(totalKmE1: number, tripKmE1?: number): void {
    this._aa?.sendOdometerData(totalKmE1, tripKmE1)
  }
  sendDrivingStatusData(status: number): void {
    this._aa?.sendDrivingStatusData(status)
  }
  sendGpsLocationData(opts: {
    latDeg: number
    lngDeg: number
    accuracyM?: number
    altitudeM?: number
    speedMs?: number
    bearingDeg?: number
  }): void {
    this._aa?.sendGpsLocationData(opts)
  }
  sendVehicleEnergyModel(
    capacityWh: number,
    currentWh: number,
    rangeM: number,
    opts?: { maxChargePowerW?: number; maxDischargePowerW?: number; auxiliaryWhPerKm?: number }
  ): void {
    this._aa?.sendVehicleEnergyModel(capacityWh, currentWh, rangeM, opts)
  }

  // Mltiple sources (mic-start, voice-session START,
  // PTT keydown) can request capture independently.
  private _startMicCapture(reason: string): void {
    if (this._micActive) return
    this._micActive = true
    if (!this._mic) {
      this._mic = new Microphone()
      this._mic.on('data', (chunk: Buffer) => {
        if (!this._micActive) return
        this._aa?.sendMicPcm(chunk)
      })
    }
    console.log(`[aaDriver] ${reason} → starting mic capture`)
    this._mic.start(5) // decodeType 5 = 16 kHz mono s16le
  }

  private _stopMicCapture(reason: string): void {
    if (!this._micActive) return
    this._micActive = false
    console.log(`[aaDriver] ${reason} → stopping mic capture`)
    this._mic?.stop()
  }

  async close(): Promise<void> {
    if (this._closed) return
    this._closed = true
    this._started = false

    this._micActive = false
    try {
      this._mic?.stop()
    } catch (err) {
      console.warn(`[aaDriver] mic stop threw: ${(err as Error).message}`)
    }
    this._mic = null

    // Best-effort graceful goodbye to the phone
    try {
      await this._aa?.requestShutdown()
    } catch (err) {
      console.warn(`[aaDriver] requestShutdown threw: ${(err as Error).message}`)
    }

    if (this._wiredBridge) {
      try {
        await this._wiredBridge.drain(500)
      } catch (err) {
        console.warn(`[aaDriver] wired bridge drain threw: ${(err as Error).message}`)
      }
    }

    try {
      this._aa?.stop()
    } catch (err) {
      console.warn(`[aaDriver] AAStack stop threw: ${(err as Error).message}`)
    }
    this._aa = null
    this._aaCfg = null

    try {
      this._wiredClientSocket?.destroy()
    } catch {
      /* already destroyed */
    }
    this._wiredClientSocket = null

    if (this._wiredBridge) {
      try {
        await this._wiredBridge.stop()
      } catch (err) {
        console.warn(`[aaDriver] wired bridge stop threw: ${(err as Error).message}`)
      }
    }
    this._wiredBridge = null
    this._wiredDevice = null

    if (this._supervisor) {
      try {
        await this._supervisor.stop()
      } catch (err) {
        console.warn(`[aaDriver] supervisor stop threw: ${(err as Error).message}`)
      }
    }
  }

  /**
   * Send a LIVI-domain message towards the phone.
   *
   * Bridges:
   *   - SendTouch         (single pointer, normalised 0..1 coordinates)
   *   - SendMultiTouch    (multi-pointer, normalised 0..1 coordinates)
   *   - SendCommand       (subset: 'frame', 'requestVideoFocus' → keyframe; rest no-op)
   *   - SendDisconnectPhone / SendCloseDongle  → ByeByeRequest(USER_SELECTION)
   *
   */
  async send(msg: SendableMessage): Promise<boolean> {
    if (!this._aa) return false

    if (msg instanceof SendTouch) {
      const usableW = this._touchW - this._touchInsetLeft - this._touchInsetRight
      const usableH = this._touchH - this._touchInsetTop - this._touchInsetBottom
      const tierX = clamp01(msg.x) * this._touchW
      const tierY = clamp01(msg.y) * this._touchH
      const ux = tierX - this._touchInsetLeft
      const uy = tierY - this._touchInsetTop
      if (ux < 0 || uy < 0 || ux >= usableW || uy >= usableH) return true
      const pointer: TouchPointer = {
        id: 0,
        x: Math.round(ux),
        y: Math.round(uy)
      }
      this._aa.sendTouch(mapTouchAction(msg.action), [pointer])
      return true
    }

    if (msg instanceof SendCommand) {
      const cmd = (msg as SendCommand).getPayload().readUInt32LE(0)
      if (DEBUG) console.log(`[INPUT] cmd=${cmd} (${CommandMapping[cmd] ?? '?'})`)

      if (cmd === CommandMapping.selectDown || cmd === CommandMapping.knobDown) {
        if (DEBUG) console.log(`[INPUT] → DPAD_CENTER press`)
        this._aa.sendButton(BUTTON_KEY.DPAD_CENTER, true)
        return true
      }
      if (cmd === CommandMapping.selectUp || cmd === CommandMapping.knobUp) {
        if (DEBUG) console.log(`[INPUT] → DPAD_CENTER release`)
        this._aa.sendButton(BUTTON_KEY.DPAD_CENTER, false)
        return true
      }

      // PTT: SEARCH (84)
      if (cmd === CommandMapping.voiceAssistant) {
        if (DEBUG) console.log(`[INPUT] → SEARCH press`)
        this._aa.sendButton(BUTTON_KEY.SEARCH, true)
        return true
      }
      if (cmd === CommandMapping.voiceAssistantRelease) {
        if (DEBUG) console.log(`[INPUT] → SEARCH release`)
        this._aa.sendButton(BUTTON_KEY.SEARCH, false)
        return true
      }

      // Rotary
      const rotaryDelta: Partial<Record<number, -1 | 1>> = {
        [CommandMapping.left]: -1,
        [CommandMapping.right]: 1,
        [CommandMapping.knobLeft]: -1,
        [CommandMapping.knobRight]: 1
      }
      const dir = rotaryDelta[cmd]
      if (dir !== undefined) {
        if (DEBUG) console.log(`[INPUT] → rotary delta=${dir > 0 ? '+1' : '-1'}`)
        this._aa.sendRotary(dir)
        return true
      }

      // TODO: replace with KEYCODE_TURN_CARD (65544) or PRIMARY/SECONDARY
      if (cmd === CommandMapping.up) {
        if (DEBUG) console.log(`[INPUT] up → DPAD_LEFT (interim tile-cycle)`)
        this._aa.sendButton(BUTTON_KEY.DPAD_LEFT, true)
        this._aa.sendButton(BUTTON_KEY.DPAD_LEFT, false)
        return true
      }
      if (cmd === CommandMapping.down) {
        if (DEBUG) console.log(`[INPUT] down → DPAD_RIGHT (interim tile-cycle)`)
        this._aa.sendButton(BUTTON_KEY.DPAD_RIGHT, true)
        this._aa.sendButton(BUTTON_KEY.DPAD_RIGHT, false)
        return true
      }

      // LIVI domain command
      const buttonMap: Partial<Record<number, number>> = {
        // System / phone
        [CommandMapping.home]: BUTTON_KEY.HOME,
        [CommandMapping.back]: BUTTON_KEY.BACK,
        [CommandMapping.acceptPhone]: BUTTON_KEY.PHONE_ACCEPT,
        [CommandMapping.rejectPhone]: BUTTON_KEY.PHONE_DECLINE,
        // Phone dialer (DTMF) keys
        [CommandMapping.phoneKey0]: BUTTON_KEY.KEY_0,
        [CommandMapping.phoneKey1]: BUTTON_KEY.KEY_1,
        [CommandMapping.phoneKey2]: BUTTON_KEY.KEY_2,
        [CommandMapping.phoneKey3]: BUTTON_KEY.KEY_3,
        [CommandMapping.phoneKey4]: BUTTON_KEY.KEY_4,
        [CommandMapping.phoneKey5]: BUTTON_KEY.KEY_5,
        [CommandMapping.phoneKey6]: BUTTON_KEY.KEY_6,
        [CommandMapping.phoneKey7]: BUTTON_KEY.KEY_7,
        [CommandMapping.phoneKey8]: BUTTON_KEY.KEY_8,
        [CommandMapping.phoneKey9]: BUTTON_KEY.KEY_9,
        [CommandMapping.phoneKeyStar]: BUTTON_KEY.KEY_STAR,
        [CommandMapping.phoneKeyHash]: BUTTON_KEY.KEY_POUND,
        [CommandMapping.phoneKeyHookSwitch]: BUTTON_KEY.HEADSETHOOK,
        // Media transport
        [CommandMapping.play]: BUTTON_KEY.MEDIA_PLAY,
        [CommandMapping.pause]: BUTTON_KEY.MEDIA_PAUSE,
        [CommandMapping.playPause]: BUTTON_KEY.MEDIA_PLAY_PAUSE,
        [CommandMapping.next]: BUTTON_KEY.MEDIA_NEXT,
        [CommandMapping.prev]: BUTTON_KEY.MEDIA_PREV
      }
      const keyCode = buttonMap[cmd]
      if (keyCode !== undefined) {
        if (DEBUG) console.log(`[INPUT] → keycode ${keyCode} press+release`)
        this._aa.sendButton(keyCode, true)
        this._aa.sendButton(keyCode, false)
        return true
      }
      if (DEBUG) console.log(`[INPUT] cmd=${cmd} not in buttonMap, no key sent`)

      switch (cmd) {
        case CommandMapping.frame:
        case CommandMapping.requestVideoFocus:
          this._emitPlugged()
          this._aa.requestKeyframe()
          setTimeout(() => this._aa?.requestKeyframe(), 500)
          return true

        case CommandMapping.releaseVideoFocus:
          return true

        case CommandMapping.requestClusterStreamFocus:
          // Maps tab opened
          this._aa.requestClusterKeyframe()
          return true

        default:
          return true
      }
    }

    if (msg instanceof SendDisconnectPhone || msg instanceof SendCloseDongle) {
      await this._aa.requestShutdown()
      return true
    }

    if (msg instanceof SendMultiTouch) {
      // SendMultiTouch carries TouchItem[] with a per-pointer action.
      if (msg.touches.length === 0) return true

      const triggerIdx = msg.touches.findIndex((t) => t.action !== MultiTouchAction.Move)
      const trigger = triggerIdx >= 0 ? msg.touches[triggerIdx]! : msg.touches[0]!
      const isMulti = msg.touches.length > 1

      let action: number
      switch (trigger.action) {
        case MultiTouchAction.Down:
          action = isMulti ? TOUCH_ACTION.POINTER_DOWN : TOUCH_ACTION.DOWN
          break
        case MultiTouchAction.Up:
          action = isMulti ? TOUCH_ACTION.POINTER_UP : TOUCH_ACTION.UP
          break
        default:
          action = TOUCH_ACTION.MOVED
      }
      const actionIndex = triggerIdx >= 0 ? triggerIdx : 0

      const usableW = this._touchW - this._touchInsetLeft - this._touchInsetRight
      const usableH = this._touchH - this._touchInsetTop - this._touchInsetBottom
      const pointers: TouchPointer[] = []
      for (const t of msg.touches) {
        const tierX = clamp01(t.x) * this._touchW
        const tierY = clamp01(t.y) * this._touchH
        const ux = tierX - this._touchInsetLeft
        const uy = tierY - this._touchInsetTop
        // Out-of-window pointer — phone has no UI under that part of the
        // canvas (AR-fit black bar / safe-area cutout). Skip silently.
        if (ux < 0 || uy < 0 || ux >= usableW || uy >= usableH) continue
        pointers.push({ id: t.id, x: Math.round(ux), y: Math.round(uy) })
      }
      if (pointers.length === 0) return true
      this._aa.sendTouch(action, pointers, actionIndex)
      return true
    }
    return false
  }
}

function clamp01(v: number): number {
  if (!Number.isFinite(v)) return 0
  if (v < 0) return 0
  if (v > 1) return 1
  return v
}

export default AaDriver
