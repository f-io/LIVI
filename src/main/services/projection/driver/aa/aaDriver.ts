/**
 * aaDriver — IPhoneDriver for native wireless Android Auto.
 * Owns AaBluetoothSupervisor (BT/Wi-Fi pairing) + AAStack (TCP 5277 protocol).
 * Translates AAStack events to LIVI domain messages; the wire protocol stays in stack/.
 */

import { EventEmitter } from 'node:events'
import { Microphone } from '@main/services/audio'
import { MessageHeader, MessageType } from '@projection/messages/common'
import {
  AudioData,
  Command,
  DongleReady,
  MediaType,
  type Message,
  MetaData,
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
import {
  AudioCommand,
  CommandMapping,
  MultiTouchAction,
  TouchAction
} from '@shared/types/ProjectionEnums'
import { computeAndroidAutoDpi, matchFittingAAResolution } from '@shared/utils'
import type { IPhoneDriver } from '../IPhoneDriver'
import { AaBluetoothSupervisor } from './aaBluetoothSupervisor'
import {
  AAStack,
  type AAStackConfig,
  type AudioChannelType,
  BUTTON_KEY,
  type MediaPlaybackMetadata,
  type MediaPlaybackStatus,
  TOUCH_ACTION,
  type TouchPointer
} from './stack/index'

/** Build a LIVI-style VideoData message from a raw H.264 NAL unit. */
function buildVideoDataMessage(buf: Buffer, width: number, height: number): VideoData {
  // VideoData wire layout (LIVI):
  //   u32 width, u32 height, u32 flags, u32 length, u32 unknown, then payload.
  //
  // IMPORTANT: must use allocUnsafeSlow (NOT allocUnsafe). ProjectionService
  // forwards `msg.data?.buffer` (the underlying ArrayBuffer) over IPC and the
  // renderer assumes the first 20 bytes are the LIVI header. allocUnsafe slices
  // from the shared 8 KB Buffer pool for small allocs, so `data.buffer` would
  // be the whole pool with arbitrary garbage in front of our header — the
  // renderer would strip 20 bytes of pool garbage and then scan ~8 KB of pool
  // for SPS patterns, hitting a false-positive on random `00 00 00 01 67…`
  // bytes (resulting in `SPS error: invalid profile_idc`).
  // allocUnsafeSlow guarantees a dedicated ArrayBuffer with byteOffset=0 and
  // exact byteLength, matching the dongle path's WebUSB-allocated ArrayBuffer.
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
 *
 * The advertised AA configs (Session.ts _buildServiceDiscoveryResponse) match
 * exactly these formats; if those ever change here must follow.
 */
const AUDIO_MAP: Record<AudioChannelType, { audioType: number; decodeType: number }> = {
  media: { audioType: 3, decodeType: 4 },
  speech: { audioType: 1, decodeType: 5 },
  phone: { audioType: 2, decodeType: 5 }
}

/** Build a LIVI AudioData message from raw PCM s16le samples. */
function buildAudioDataMessage(buf: Buffer, channel: AudioChannelType): AudioData {
  // AudioData wire layout (LIVI readable.ts):
  //   u32 LE decodeType (selects sample rate / channel count / format)
  //   f32 LE volume     (0 = use system default; we don't volume-shape here)
  //   u32 LE audioType  (1=SPEECH, 2=SYSTEM, 3=MEDIA — drives routing)
  //   …Int16Array s16le payload (must be Int16-aligned — AA always sends 16-bit)
  const { audioType, decodeType } = AUDIO_MAP[channel]
  const HEADER = 12
  // Ensure even length so it can be reinterpreted as Int16Array on read.
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
 * Build a LIVI AudioData "command" message for stream lifecycle events.
 *
 * ProjectionAudio gates incoming PCM behind per-stream `*Active` flags
 * (mediaActive, siriActive, …) that flip on AudioCommand.* messages. In the
 * dongle path the dongle synthesises these; the AA wire protocol has no
 * equivalent message — instead each AV channel announces start/stop via its
 * own AV_MSG.START_INDICATION / STOP_INDICATION. We translate those here.
 *
 * Wire layout (1-byte payload after the 12-byte header):
 *   u32 LE decodeType
 *   f32 LE volume
 *   u32 LE audioType
 *   u8     command          ← AudioCommand enum value
 *
 * AudioData.constructor(readable.ts) recognises payloadBytes==1 as a command.
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
 * Build a LIVI `MetaData(MediaType.Data)` domain message wrapping a JSON
 * payload of NowPlaying fields. LIVI's `MediaData` constructor expects
 * `[u32 LE innerType][JSON UTF-8…\0]` and parses up to `length - 1` bytes
 * (the trailing NUL byte mirrors what the dongle sends).
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
 * Build a LIVI `MetaData(MediaType.AlbumCover)` domain message wrapping raw
 * album-art bytes (typically JPEG or PNG). LIVI's `MediaData` auto-detects
 * raw vs. base64 and re-encodes as base64 for the renderer either way.
 */
function buildAlbumArtMessage(albumArt: Buffer): MetaData {
  const data = Buffer.allocUnsafeSlow(4 + albumArt.length)
  data.writeUInt32LE(MediaType.AlbumCover, 0)
  albumArt.copy(data, 4)
  const header = new MessageHeader(data.length, MessageType.MetaData)
  return new MetaData(header, data)
}

/**
 * Map an AA audio-channel start/stop transition to the corresponding LIVI
 * AudioCommand. The mapping mirrors what the dongle emits so ProjectionAudio
 * doesn't need to know the source.
 *
 *   media  → AudioMediaStart / AudioMediaStop      (Spotify, YouTube Music, …)
 *   speech → AudioNaviStart  / AudioNaviStop       (Maps voice, voice assist replies)
 *   phone  → AudioOutputStart / AudioOutputStop    (system notifications)
 *
 * SPEECH could equally well be AudioSiriStart on certain phones, but Navi is
 * the conservative pick — it routes to ProjectionAudio's `nav` stream which
 * ducks music underneath, matching what users expect from prompts.
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
 * Map a LIVI single-pointer TouchAction to aasdk's PointerAction enum.
 * NB: aasdk's enum is DOWN=0, UP=1, MOVED=2 — the legacy {DOWN:0, MOVE:1, UP:2}
 * we used previously had Move and Up swapped, so taps registered as releases
 * and drags as presses on the phone.
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

/** Map a LIVI MultiTouchAction (per-pointer) to aasdk's PointerAction enum. */
function mapMultiTouchAction(action: MultiTouchAction): number {
  switch (action) {
    case MultiTouchAction.Down:
      return TOUCH_ACTION.DOWN
    case MultiTouchAction.Move:
      return TOUCH_ACTION.MOVED
    case MultiTouchAction.Up:
      return TOUCH_ACTION.UP
  }
  return TOUCH_ACTION.MOVED
}

export interface AaDriverOptions {
  /** Override BT supervisor — useful for tests / non-Linux dry-runs. */
  supervisor?: AaBluetoothSupervisor | null
}

export class AaDriver extends EventEmitter implements IPhoneDriver {
  private _aa: AAStack | null = null
  private _supervisor: AaBluetoothSupervisor | null
  private _started = false
  private _closed = false
  /** Touchscreen pixel space we advertised to the phone — used to denormalise SendTouch coords. */
  private _touchW = 1280
  private _touchH = 720
  /** Mic capture instance, lifecycle-bound to phone OPEN_REQUEST/STOP_INDICATION. */
  private _mic: Microphone | null = null
  private _micActive = false

  constructor(opts: AaDriverOptions = {}) {
    super()
    // Cap auto-restarts so a deterministic crash (stale BT profile, missing
    // dependency, sudoers regression, …) doesn't fall into an infinite spawn
    // loop spamming the log. Five tries is enough to ride out a transient
    // wlan0/NM race; beyond that we want a visible "give up" so the user
    // sees what's actually wrong instead of a wall of repeated tracebacks.
    this._supervisor = opts.supervisor ?? new AaBluetoothSupervisor({ maxRestarts: 5 })
  }

  async start(cfg: DongleConfig): Promise<boolean> {
    if (this._started) return true
    this._started = true
    this._closed = false

    // 1. Bring up python BT/Wi-Fi stack first — it owns the AP the phone will
    //    join. The TCP listener can come up in parallel; AA flow only completes
    //    once the phone is actually on the AP.
    if (this._supervisor) {
      this._supervisor.on('stdout', (line) => console.log(`[aa-bt] ${line}`))
      this._supervisor.on('stderr', (line) => console.warn(`[aa-bt!] ${line}`))
      this._supervisor.on('error', (err) => {
        console.warn(`[aaDriver] supervisor error: ${err.message}`)
      })
      this._supervisor.start(cfg)
    }

    // 2. AAStackConfig is FLAT (matches LIVI's DongleConfig style).
    //    btMacAddress / wifiBssid are auto-detected from sysfs by the stack.
    //
    // Resolution strategy (matches openauto default + dongle behaviour):
    //   - Pick the AA tier (800 / 1280 / 1920 / 2560 / 3840 wide).
    //   - Advertise that full tier as videoWidth/videoHeight, with
    //     width_margin = height_margin = 0 in the SDR. Phone always renders
    //     into the full tier — no phone-side padding.
    //   - LIVI renderer-side already does symmetric cropLeft/cropTop based
    //     on `matchFittingAAResolution(settings)` versus negotiatedWidth/
    //     Height (= tier here), so display-AR mismatches are handled in
    //     the renderer pipeline (Projection.tsx near line 1019).
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
      // Pass through so the stack's SDR can compute pixel_aspect_ratio_e4
      // — the phone pre-distorts its UI rendering when display AR ≠ tier AR
      // so the HU's per-axis stretch onto the actual display ends up
      // AR-correct. No black bars, no crop, no letterbox.
      displayWidth: cfg.width,
      displayHeight: cfg.height,
      // HandDriveType.LHD=0 / RHD=1 maps 1:1 to AA DriverPosition.
      driverPosition: cfg.hand === 1 ? 1 : 0,
      wifiSsid: name,
      wifiPassword: cfg.wifiPassword || '12345678',
      wifiChannel: cfg.wifiChannel
    }
    const displayAR = cfg.width / cfg.height
    const tierAR = tierW / tierH
    const parE4 = displayAR !== tierAR ? Math.round((displayAR / tierAR) * 10000) : 10000
    console.log(
      `[aaDriver] display ${cfg.width}×${cfg.height} (AR ${displayAR.toFixed(3)}) → ` +
        `AA tier ${tierW}×${tierH} (AR ${tierAR.toFixed(3)}) @${aaDpi}dpi, PAR e4=${parE4}`
    )

    // Touch denorm base = full tier (= negotiated stream space). Renderer's
    // useCarplayMultiTouch already returns coords normalised over this
    // space, no padding offset to add.
    this._touchW = tierW
    this._touchH = tierH

    const aa = new AAStack(aaCfg)
    this._aa = aa

    aa.on('connected', () => {
      console.log('[aaDriver] AAStack connected → DongleReady + Plugged(AndroidAuto)')
      // DongleReady has no payload.
      const readyHdr = new MessageHeader(0, MessageType.Open)
      this.emit('message', new DongleReady(readyHdr) as Message)
      this._emitPlugged()
    })

    aa.on('disconnected', (reason?: string) => {
      // IMPORTANT: AA-native treats TCP disconnect as TRANSIENT, not "unplugged".
      // The phone may retry within seconds (RFCOMM → AP-join → TCP reconnect).
      // The python BT/Wi-Fi supervisor MUST stay alive across this — if we emit
      // Unplugged here, ProjectionService calls stop() → driver.close() →
      // supervisor.stop() → SIGTERM → hostapd dies → wlan0 ENABLED->DISABLED,
      // and the phone's retry attempts hit a dead AP.
      //
      // We only log the disconnect; the next AAStack 'connected' will fire
      // Plugged again. True teardown (real Unplugged) happens via close().
      console.log(
        `[aaDriver] AAStack disconnected (${reason ?? 'no reason'}) — supervisor stays up for retry`
      )
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

    // Mic lifecycle: phone opens / closes its mic-input channel. We spawn a
    // gst-launch capture (decodeType=5 → 16 kHz mono s16le, matches what we
    // advertised in the SDR) and pump every PCM chunk straight to the active
    // session. AAStack handles flow control + max_unacked pacing internally.
    aa.on('mic-start', () => {
      if (this._micActive) return
      this._micActive = true
      if (!this._mic) {
        this._mic = new Microphone()
        this._mic.on('data', (chunk: Buffer) => {
          if (!this._micActive) return
          this._aa?.sendMicPcm(chunk)
        })
      }
      console.log('[aaDriver] mic-start → starting capture (16 kHz mono)')
      this._mic.start(5) // decodeType 5 = 16 kHz mono s16le
    })

    aa.on('mic-stop', () => {
      if (!this._micActive) return
      this._micActive = false
      console.log('[aaDriver] mic-stop → stopping capture')
      this._mic?.stop()
    })

    // Phone asked HU to swap to its native (host) UI. Surface this as a
    // Command(requestHostUI) so the renderer's Projection.tsx — which already
    // listens for that command via its IPC bridge — calls gotoHostUI() and
    // navigates to /media. The AA session itself stays running in the
    // background; the phone will re-request PROJECTED focus to come back.
    aa.on('host-ui-requested', () => {
      console.log('[aaDriver] host-ui-requested → emitting Command(requestHostUI)')
      const buf = Buffer.allocUnsafe(4)
      buf.writeUInt32LE(CommandMapping.requestHostUI, 0)
      const header = new MessageHeader(buf.length, MessageType.Command)
      this.emit('message', new Command(header, buf) as Message)
    })

    aa.on('media-metadata', (m: MediaPlaybackMetadata) => {
      // MediaPlaybackMetadata (track info) → LIVI MediaData JSON keys.
      // album_art goes into its own MetaData(AlbumCover) message — LIVI's
      // payload format keeps art separate, the renderer recombines from
      // mediaData.json. Note: MediaAPPName ("source app", e.g. Spotify) is
      // NOT here — it comes through MediaPlaybackStatus.media_source.
      const media: Record<string, unknown> = {}
      if (m.song !== undefined) media.MediaSongName = m.song
      if (m.artist !== undefined) media.MediaArtistName = m.artist
      if (m.album !== undefined) media.MediaAlbumName = m.album
      // LIVI renderer treats MediaSongDuration/PlayTime as milliseconds (the
      // CarPlay dongle reports them that way). AA's MediaPlaybackMetadata
      // sends them in seconds, so scale × 1000 here.
      if (m.durationSeconds !== undefined) media.MediaSongDuration = m.durationSeconds * 1000
      if (Object.keys(media).length > 0) {
        this.emit('message', buildMediaJsonMessage(media) as Message)
      }
      if (m.albumArt && m.albumArt.length > 0) {
        this.emit('message', buildAlbumArtMessage(m.albumArt) as Message)
      }
    })

    aa.on('media-status', (s: MediaPlaybackStatus) => {
      // MediaPlaybackStatus (per-tick playback state) → LIVI MediaData JSON.
      // Phone emits this every ~1 s while playback is active, so the
      // progress bar and play/pause icon stay in sync.
      //   state            → MediaPlayStatus  (1 = playing, 0 = anything else)
      //   media_source     → MediaAPPName     (e.g. "com.spotify.music")
      //   playback_seconds → MediaSongPlayTime
      const playStatus = s.state === 'playing' ? 1 : 0
      const media: Record<string, unknown> = { MediaPlayStatus: playStatus }
      if (s.mediaSource !== undefined) media.MediaAPPName = s.mediaSource
      // ms — see comment in media-metadata listener.
      if (s.playbackSeconds !== undefined) media.MediaSongPlayTime = s.playbackSeconds * 1000
      this.emit('message', buildMediaJsonMessage(media) as Message)
    })

    aa.on('error', (err: Error) => {
      // Suppress error spam while we're tearing down — the python child has
      // been killed, the TCP socket on the phone side resets, and these late
      // EPIPE/ECONNRESET events otherwise reach ProjectionService AFTER
      // webContents is already destroyed (TypeError: Object has been destroyed).
      if (this._closed) {
        console.debug(`[aaDriver] suppressed AAStack error during close: ${err.message}`)
        return
      }
      // Transient socket-level errors mid-session are normal: the phone
      // TCP-resets for many reasons (encoder restart on resolution / focus
      // change, app switch, transient AP roam, etc.). Treating those as
      // a UI-level "failure" causes setAaActive(false → true) flips that
      // unmount/remount streaming-overlay state and leave the renderer's
      // VideoDecoder pinned to a now-stale SPS. Just log; the next
      // 'connected' will re-arm via the Plugged path. Real fatal errors
      // (server bind failure, spawn errors) are surfaced by the supervisor
      // exit-loop and the python stderr handler instead.
      console.warn(`[aaDriver] AAStack transient error: ${err.message}`)
    })

    aa.start()
    console.log('[aaDriver] AA stack listening on TCP 5277')
    return true
  }

  /**
   * Emit a `Plugged{phoneType=AndroidAuto, wifi=1}` LIVI domain message.
   * Used both at initial AA connect and on every renderer-mount keyframe
   * request, so the renderer can re-enter the projection-active UI state
   * after a host-UI excursion (where ProjectionAudio's mediaActive flag and
   * the renderer's isStreaming flag drift to the "no phone" defaults).
   */
  private _emitPlugged(): void {
    const pluggedBuf = Buffer.allocUnsafe(8)
    pluggedBuf.writeUInt32LE(PhoneType.AndroidAuto, 0)
    pluggedBuf.writeUInt32LE(1, 4) // wifi available
    const pluggedHdr = new MessageHeader(pluggedBuf.length, MessageType.Plugged)
    this.emit('message', new Plugged(pluggedHdr, pluggedBuf) as Message)
  }

  async close(): Promise<void> {
    if (this._closed) return
    this._closed = true
    this._started = false

    // Tear mic capture down before the session — we don't want to be holding
    // the mic device once AA is gone.
    this._micActive = false
    try {
      this._mic?.stop()
    } catch (err) {
      console.warn(`[aaDriver] mic stop threw: ${(err as Error).message}`)
    }
    this._mic = null

    // Best-effort graceful goodbye to the phone (idempotent — no-op if the
    // session is already CLOSED). We don't wait for an ack; the next steps
    // tear the socket down anyway.
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
   * Currently bridges:
   *   - SendTouch         (single pointer, normalised 0..1 coordinates)
   *   - SendMultiTouch    (multi-pointer, normalised 0..1 coordinates)
   *   - SendCommand       (subset: 'frame', 'requestVideoFocus' → keyframe; rest no-op)
   *   - SendDisconnectPhone / SendCloseDongle  → ByeByeRequest(USER_SELECTION)
   *
   * Normalised touch coordinates are denormalised into the touchscreen pixel
   * space we advertised to the phone (videoWidth × videoHeight for standard
   * tiers). Anything else falls through to `return false` so the caller can
   * surface the no-op.
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
      // SendCommand is the dongle's catch-all for HU→Phone control messages.
      // We translate the few that have AA equivalents and silently accept the
      // rest (return true) so ProjectionService doesn't log spurious failures
      // for dongle-only commands like 'wifi24g', 'audioTransferOn', …
      const cmd = (msg as SendCommand).getPayload().readUInt32LE(0)

      // LIVI domain command → AA HW key event mapping.
      //
      // Tap-style commands (selectDown/selectUp emit DPAD_CENTER as a paired
      // press/release) get a clean DOWN+UP burst here; longer-held semantics
      // (e.g. "hold home for assistant") aren't represented in CommandMapping
      // anyway, so a fire-and-forget pair is the right model.
      // CommandMapping → Android KeyEvent.KEYCODE_* mapping. This is the
      // default routing for LIVI's built-in commands; user-side rebinds in
      // the LIVI key-mapper happen upstream and arrive here as the same
      // CommandMapping enum, so no per-user logic needed in the driver.
      //
      // AA exposes two distinct nav input "families":
      //   • DPAD_{LEFT,RIGHT,UP,DOWN,CENTER} — touch-equivalent directional
      //     input. Phone-app side this is what most apps map to in-app
      //     navigation (next song, scroll list, focus button).
      //   • NAVIGATE_{PREVIOUS,NEXT,IN,OUT} — rotary controller events.
      //     Used by AA's rotary-aware widgets (Settings, Contacts).
      // We pick DPAD as the default for the generic arrow / select commands
      // (they're closer to "I tapped on screen" semantics) and route the
      // explicit knob commands to NAVIGATE_*. The user's keymapper can swap
      // any of these per binding.
      // Sending both families simultaneously was tried and reverted: the
      // phone treats multiple keys in the same KeyEvent as modifier-style
      // simultaneous press, not as "alternative", so list nav fires *and*
      // tab switch fires on every press — net result was unusable.
      const buttonMap: Partial<Record<number, number>> = {
        // Generic arrow keys → D-PAD
        [CommandMapping.left]: BUTTON_KEY.DPAD_LEFT,
        [CommandMapping.right]: BUTTON_KEY.DPAD_RIGHT,
        [CommandMapping.up]: BUTTON_KEY.DPAD_UP,
        [CommandMapping.down]: BUTTON_KEY.DPAD_DOWN,
        [CommandMapping.selectDown]: BUTTON_KEY.DPAD_CENTER,
        [CommandMapping.selectUp]: BUTTON_KEY.DPAD_CENTER,
        // Rotary controller (Audi MMI / Mercedes COMAND style) → NAVIGATE_*
        [CommandMapping.knobLeft]: BUTTON_KEY.NAVIGATE_PREVIOUS,
        [CommandMapping.knobRight]: BUTTON_KEY.NAVIGATE_NEXT,
        [CommandMapping.knobUp]: BUTTON_KEY.NAVIGATE_PREVIOUS,
        [CommandMapping.knobDown]: BUTTON_KEY.NAVIGATE_NEXT,
        // System / phone
        [CommandMapping.home]: BUTTON_KEY.HOME,
        [CommandMapping.back]: BUTTON_KEY.BACK,
        [CommandMapping.acceptPhone]: BUTTON_KEY.PHONE_ACCEPT,
        [CommandMapping.rejectPhone]: BUTTON_KEY.PHONE_DECLINE,
        [CommandMapping.siri]: BUTTON_KEY.VOICE_ASSIST,
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
        this._aa.sendButton(keyCode, true)
        this._aa.sendButton(keyCode, false)
        return true
      }

      switch (cmd) {
        case CommandMapping.frame:
        case CommandMapping.requestVideoFocus:
          // Renderer (re)mounted, or LIVI signals "projection surface visible
          // again". Two things have to happen:
          //   1) Replay Plugged so ProjectionService re-arms its `lastPluggedPhoneType`
          //      and the renderer re-enters the projection-active UI state
          //      (tab highlight, isStreaming gating). After a host-UI excursion
          //      the renderer drifts to streaming=off until it sees a fresh
          //      resolution event; without Plugged we'd never get one because
          //      the AA session never actually "unplugged".
          //   2) Ask the phone for a fresh IDR so the decoder doesn't have to
          //      wait for the next scheduled intra-refresh (same VideoFocusIndication
          //      mechanism aasdk uses).
          //
          // The second VideoFocusIndication ~500 ms later works around a Phone-
          // side issue we see after a NATIVE→PROJECTED switch: the first
          // keyframe request right after re-entry occasionally lands while the
          // encoder is still in its idle/native state and gets coalesced into
          // a no-op, leaving the renderer staring at a black frame until the
          // next scheduled IDR (which can take many seconds, or never if the
          // phone UI hasn't redrawn). The second nudge falls into the now-
          // active encoder window and reliably yields a fresh IDR. Same
          // pattern is used by several headless AA implementations.
          this._emitPlugged()
          this._aa.requestKeyframe()
          setTimeout(() => this._aa?.requestKeyframe(), 500)
          return true

        case CommandMapping.releaseVideoFocus:
          // We could send VideoFocusIndication(NATIVE) to tell the phone we're
          // backgrounded, but the phone doesn't *need* it — projection keeps
          // running and the next requestVideoFocus pulls a fresh IDR. Treat
          // as no-op so the user's quick tab toggles don't churn the encoder.
          return true

        default:
          // Unknown / dongle-specific commands: accept silently. The dongle
          // path needs them; we don't, and returning false would surface as
          // "[ProjectionService] driver.send failed" noise.
          return true
      }
    }

    if (msg instanceof SendDisconnectPhone || msg instanceof SendCloseDongle) {
      // Both arrive from ProjectionService.disconnectPhone() during app exit
      // / "Stop AA" UI action. The AA equivalent is ByeByeRequest with
      // USER_SELECTION, which we ask the active session to send. Supervisor
      // teardown (hostapd / dnsmasq / BT) is handled by aaDriver.close()
      // afterwards in the same sequence ProjectionService.stop() drives.
      this._aa.requestShutdown()
      return true
    }

    if (msg instanceof SendMultiTouch) {
      // SendMultiTouch carries TouchItem[] internally with action per pointer,
      // but the AA wire wants a single touchAction that applies to the whole
      // event plus N pointer descriptors. We pick the action from the first
      // item: AA semantics are that DOWN/UP fire on transitions and MOVE while
      // any pointer is held — input handlers in LIVI build SendMultiTouch the
      // same way, so this is consistent with what the renderer emits.
      if (msg.touches.length === 0) return true
      const first = msg.touches[0]!
      const action = mapMultiTouchAction(first.action)
      const pointers: TouchPointer[] = msg.touches.map((t) => ({
        id: t.id,
        x: Math.round(clamp01(t.x) * this._touchW),
        y: Math.round(clamp01(t.y) * this._touchH)
      }))
      this._aa.sendTouch(action, pointers)
      return true
    }

    // Heartbeats, file commands, etc. — not relevant for the native AA path.
    // Returning false (no-op) is honest; ProjectionService logs accordingly.
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
