/**
 * aaDriver — IPhoneDriver for native wireless Android Auto.
 * Owns AaBluetoothSupervisor (BT/Wi-Fi pairing) + AAStack (TCP 5277 protocol).
 * Translates AAStack events to LIVI domain messages.
 */

import { EventEmitter } from 'node:events'
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
import { computeAndroidAutoDpi, matchFittingAAResolution } from '@shared/utils'
import type { IPhoneDriver } from '../IPhoneDriver'
import { AaBluetoothSupervisor } from './aaBluetoothSupervisor'
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
  type TouchPointer
} from './stack/index'

/** Build VideoData message from a raw H.264 NAL unit. */
function buildVideoDataMessage(buf: Buffer, width: number, height: number): VideoData {
  // VideoData wire layout (LIVI):
  //   u32 width, u32 height, u32 flags, u32 length, u32 unknown, then payload.
  const HEADER = 20
  const data = Buffer.allocUnsafeSlow(HEADER + buf.length)
  data.writeUInt32LE(width, 0)
  data.writeUInt32LE(height, 4)
  data.writeUInt32LE(0, 8) // flags
  data.writeUInt32LE(buf.length, 12)
  data.writeUInt32LE(0, 16) // unknown
  buf.copy(data, HEADER)
  const header = new MessageHeader(data.length, MessageType.VideoData)
  return new VideoData(header, data)
}

/**
 * AA channel type → LIVI audio classifier mapping.
 *   'media'  → MEDIA   (audioType=3) at 48 kHz stereo s16le → decodeType 4
 *   'speech' → SPEECH  (audioType=1) at 16 kHz mono s16le   → decodeType 5
 *   'phone'  → SYSTEM  (audioType=2) at 16 kHz mono s16le   → decodeType 5
 */
const AUDIO_MAP: Record<AudioChannelType, { audioType: number; decodeType: number }> = {
  media: { audioType: 3, decodeType: 4 },
  speech: { audioType: 1, decodeType: 5 },
  phone: { audioType: 2, decodeType: 5 }
}

/** Build AudioData message from raw PCM s16le samples. */
function buildAudioDataMessage(buf: Buffer, channel: AudioChannelType): AudioData {
  // AudioData wire layout (LIVI readable.ts):
  //   u32 LE decodeType (selects sample rate / channel count / format)
  //   f32 LE volume     (0 = use system default; we don't volume-shape here)
  //   u32 LE audioType  (1=SPEECH, 2=SYSTEM, 3=MEDIA — drives routing)
  //   …Int16Array s16le payload (must be Int16-aligned — AA always sends 16-bit)
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

/**
 * Build AudioData "command" message for stream lifecycle events.
 *
 * Wire layout (1-byte payload after the 12-byte header):
 *   u32 LE decodeType
 *   f32 LE volume
 *   u32 LE audioType
 *   u8     command          - AudioCommand enum value
 *
 */
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

/**
 * Build MetaData(MediaType.Data)
 */
function buildMediaJsonMessage(media: Record<string, unknown>): MetaData {
  const json = JSON.stringify(media)
  const payload = Buffer.from(json + '\0', 'utf8')
  const data = Buffer.allocUnsafeSlow(4 + payload.length)
  data.writeUInt32LE(MediaType.Data, 0)
  payload.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

/**
 * Build MetaData(MediaType.AlbumCover)
 */
function buildAlbumArtMessage(albumArt: Buffer): MetaData {
  const data = Buffer.allocUnsafeSlow(4 + albumArt.length)
  data.writeUInt32LE(MediaType.AlbumCover, 0)
  albumArt.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

/**
 * Build MetaData(NavigationMetaType.DashboardInfo) — Carlinkit-shaped NaviBag
 * JSON. Drops into the existing ProjectionService navigation pipeline.
 */
function buildNaviJsonMessage(navi: Record<string, unknown>): MetaData {
  const json = JSON.stringify(navi)
  const payload = Buffer.from(json + '\0', 'utf8')
  const data = Buffer.allocUnsafeSlow(4 + payload.length)
  data.writeUInt32LE(NavigationMetaType.DashboardInfo, 0)
  payload.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

/**
 * Build MetaData(NavigationMetaType.DashboardImage) — turn-icon bitmap.
 * The image bytes are forwarded verbatim; renderer picks up base64 in NaviBag.
 */
function buildNaviImageMessage(image: Buffer): MetaData {
  const data = Buffer.allocUnsafeSlow(4 + image.length)
  data.writeUInt32LE(NavigationMetaType.DashboardImage, 0)
  image.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

/**
 * Map an AA audio-channel start/stop transition to the corresponding LIVI
 * AudioCommand.
 *
 *   media  → AudioMediaStart / AudioMediaStop      (Spotify, YouTube Music, …)
 *   speech → AudioNaviStart  / AudioNaviStop       (Maps voice, voice assist replies)
 *   phone  → AudioOutputStart / AudioOutputStop    (system notifications)
 *
 */
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
 * Map a single-pointer TouchAction to PointerAction enum.
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
}

export class AaDriver extends EventEmitter implements IPhoneDriver {
  private _aa: AAStack | null = null
  private _supervisor: AaBluetoothSupervisor | null
  private _started = false
  private _closed = false
  private _touchW = 1280
  private _touchH = 720
  private _mic: Microphone | null = null
  private _micActive = false

  // Accumulating NaviBag — flushed via buildNaviJsonMessage on every patch.
  private _naviBag: Record<string, unknown> = {}
  private _naviActive = false
  private _naviApp: string | undefined

  constructor(opts: AaDriverOptions = {}) {
    super()
    // Cap auto-restarts so a deterministic crash (stale BT profile, missing
    // dependency, sudoers regression, …)
    this._supervisor = opts.supervisor ?? new AaBluetoothSupervisor({ maxRestarts: 5 })
  }

  async start(cfg: DongleConfig): Promise<boolean> {
    if (this._started) return true
    this._started = true
    this._closed = false

    // 1. Bring up python BT/Wi-Fi stack first
    if (this._supervisor) {
      this._supervisor.on('stdout', (line) => console.log(`[aa-bt] ${line}`))
      this._supervisor.on('stderr', (line) => console.warn(`[aa-bt!] ${line}`))
      this._supervisor.on('error', (err) => {
        console.warn(`[aaDriver] supervisor error: ${err.message}`)
      })
      this._supervisor.start(cfg)
    }

    // 2. AAStackConfig
    //
    // Resolution strategy:
    //   - Pick the AA tier (800 / 1280 / 1920 / 2560 / 3840 wide).
    //   - Advertise that full tier as videoWidth/videoHeight, with
    //     width_margin = height_margin = 0 in the SDR. Phone always renders
    //     into the full tier.
    //   - LIVI renderer-side already does symmetric cropLeft/cropTop based
    //     on `matchFittingAAResolution(settings)` versus negotiatedWidth/
    //     Height (= tier here), so display-AR mismatches are handled in
    //     the renderer pipeline.
    //   - Touch coords therefore use the full tier as denormalisation
    //     base — that's what `useCarplayMultiTouch` already produces for
    //     this layout (streamWidth/streamHeight = negotiated).
    //   - DPI scales with the tier.
    //
    const tierW =
      cfg.width >= 3840
        ? 3840
        : cfg.width >= 2560
          ? 2560
          : cfg.width >= 1920
            ? 1920
            : cfg.width >= 1280
              ? 1280
              : 800
    const tierH =
      tierW === 3840
        ? 2160
        : tierW === 2560
          ? 1440
          : tierW === 1920
            ? 1080
            : tierW === 1280
              ? 720
              : 480
    const aaDpi = computeAndroidAutoDpi(tierW, tierH)
    const name = cfg.carName?.trim() ? cfg.carName : 'LIVI'
    const aaCfg: AAStackConfig = {
      huName: name,
      videoWidth: tierW,
      videoHeight: tierH,
      videoDpi: aaDpi,
      videoFps: cfg.fps === 60 ? 60 : 30,
      displayWidth: cfg.width,
      displayHeight: cfg.height,
      driverPosition: cfg.hand === 1 ? 1 : 0,
      wifiSsid: name,
      wifiPassword: cfg.wifiPassword || '12345678',
      wifiChannel: cfg.wifiChannel,
      fuelTypes: mapCarTypeToFuelTypes(cfg.carType),
      evConnectorTypes: cfg.evConnectorTypes
    }
    const displayAR = cfg.width / cfg.height
    const tierAR = tierW / tierH
    const parE4 = displayAR !== tierAR ? Math.round((displayAR / tierAR) * 10000) : 10000
    console.log(
      `[aaDriver] display ${cfg.width}×${cfg.height} (AR ${displayAR.toFixed(3)}) → ` +
        `AA tier ${tierW}×${tierH} (AR ${tierAR.toFixed(3)}) @${aaDpi}dpi, PAR e4=${parE4}`
    )

    this._touchW = tierW
    this._touchH = tierH

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
      // Drop any accumulated nav state so the next session starts fresh.
      this._naviBag = {}
      this._naviActive = false
      this._naviApp = undefined
    })

    aa.on('video-frame', (buf: Buffer, _ts: bigint) => {
      const w = aaCfg.videoWidth ?? 1280
      const h = aaCfg.videoHeight ?? 720
      this.emit('message', buildVideoDataMessage(buf, w, h) as Message)
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

    // Navigation (turn-by-turn) — accumulate state + flush a NaviBag whenever
    // any field changes, so the existing ProjectionService nav pipeline gets
    // the same shape it'd see from the dongle.
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

    aa.start()
    console.log('[aaDriver] AA stack listening on TCP 5277')
    return true
  }

  /**
   * Merge a NaviBag patch into the accumulated state and emit the full bag as
   * MetaData(DashboardInfo). ProjectionService runs it through normalize +
   * translate, so partial updates flow correctly into the UI.
   */
  private _publishNavi(patch: Record<string, unknown>): void {
    Object.assign(this._naviBag, patch)
    if (this._naviApp !== undefined && this._naviBag.NaviAPPName === undefined) {
      this._naviBag.NaviAPPName = this._naviApp
    }
    this.emit('message', buildNaviJsonMessage(this._naviBag) as Message)
  }

  /**
   * Emit a `Plugged{phoneType=AndroidAuto, wifi=1}` LIVI domain message.
   */
  private _emitPlugged(): void {
    const pluggedBuf = Buffer.allocUnsafe(8)
    pluggedBuf.writeUInt32LE(PhoneType.AndroidAuto, 0)
    pluggedBuf.writeUInt32LE(1, 4) // wifi available
    const pluggedHdr = new MessageHeader(pluggedBuf.length, MessageType.Plugged)
    this.emit('message', new Plugged(pluggedHdr, pluggedBuf) as Message)
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
      this._aa?.requestShutdown()
    } catch (err) {
      console.warn(`[aaDriver] requestShutdown threw: ${(err as Error).message}`)
    }

    try {
      this._aa?.stop()
    } catch (err) {
      console.warn(`[aaDriver] AAStack stop threw: ${(err as Error).message}`)
    }
    this._aa = null

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
      const pointer: TouchPointer = {
        id: 0,
        x: Math.round(clamp01(msg.x) * this._touchW),
        y: Math.round(clamp01(msg.y) * this._touchH)
      }
      this._aa.sendTouch(mapTouchAction(msg.action), [pointer])
      return true
    }

    if (msg instanceof SendCommand) {
      const cmd = (msg as SendCommand).getPayload().readUInt32LE(0)
      if (DEBUG) console.log(`[INPUT] cmd=${cmd} (${CommandMapping[cmd] ?? '?'})`)

      // selectDown/selectUp and knobDown/knobUp arrive as press/release pairs
      // from the renderer (see useKeyDown.ts — ~200ms between them). They ARE
      // the press and release of a center button (Enter / rotary push).
      // Don't run them through the buttonMap path below, which fires a full
      // press+release per event and would generate a double-click on the phone.
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

      // Rotary semantics for the rotation axis: left/right + knob rotation
      // → rotary delta (in-container walk). A real rotary controller has
      // only this one rotation axis.
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

      // LIVI domain command → AA HW key event mapping. Arrow keys + knob
      // rotation handled above (dual DPAD+rotary / pure rotary). selectDown/Up
      // and knobDown/Up handled above as DPAD_CENTER press/release pairs.
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

        default:
          return true
      }
    }

    if (msg instanceof SendDisconnectPhone || msg instanceof SendCloseDongle) {
      this._aa.requestShutdown()
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

      const pointers: TouchPointer[] = msg.touches.map((t) => ({
        id: t.id,
        x: Math.round(clamp01(t.x) * this._touchW),
        y: Math.round(clamp01(t.y) * this._touchH)
      }))
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
