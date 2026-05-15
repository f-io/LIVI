/**
 * AA wireless session — one per TCP connection.
 * State: INIT → VERSION → TLS_HANDSHAKE → AUTH → SERVICE_DISCOVERY
 *        → CHANNEL_SETUP → RUNNING → CLOSED
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import { DEBUG, TRACE } from '@main/constants'
import { AudioChannel, type AudioChannelType } from '../channels/AudioChannel.js'
import { InputChannel, type TouchPointer } from '../channels/InputChannel.js'
import {
  MediaInfoChannel,
  type MediaPlaybackMetadata,
  type MediaPlaybackStatus
} from '../channels/MediaInfoChannel.js'
import { MicChannel } from '../channels/MicChannel.js'
import {
  NavigationChannel,
  type NavigationDistanceUpdate,
  type NavigationStatusUpdate,
  type NavigationTurnUpdate
} from '../channels/NavigationChannel.js'
import { decodeStart, fieldFloat, fieldLenDelim, fieldVarint } from '../channels/protoEnc.js'
import { VideoChannel } from '../channels/VideoChannel.js'
import {
  AUDIO_TYPE,
  AV_MSG,
  AV_SETUP_STATUS,
  AV_STREAM_TYPE,
  BT_PAIRING_METHOD,
  CH,
  COLOR_SCHEME,
  CTRL_MSG,
  DISPLAY_TYPE,
  FRAME_FLAGS,
  MEDIA_CODEC,
  SENSOR_TYPE,
  STATUS_OK,
  VERSION,
  VIDEO_FPS,
  VIDEO_RESOLUTION
} from '../constants.js'
import { encodeFrame, FrameParser, type RawFrame } from '../frame/codec.js'
import { decode, encode, loadProtos, type ProtoTypes } from '../proto/index.js'
import { ControlChannel } from './ControlChannel.js'
import { buildServiceDiscoveryResponse } from './ServiceDiscoveryBuilder.js'
import { SessionTls } from './SessionTls.js'

/** Per-frame chatter is suppressed for these channels under DEBUG=1.
 *  Set TRACE=1 to see them anyway. SENSOR is included because the phone
 *  walks ~15 sensor types at session start, doubling the per-message log. */
const isFrameChannel = (ch: number): boolean =>
  ch === CH.VIDEO ||
  ch === CH.CLUSTER_VIDEO ||
  ch === CH.MEDIA_AUDIO ||
  ch === CH.SPEECH_AUDIO ||
  ch === CH.SYSTEM_AUDIO ||
  ch === CH.INPUT ||
  ch === CH.MIC_INPUT ||
  ch === CH.SENSOR

/** Ping/pong on the control channel runs every 1500 ms in both directions.
 *  Same idea as isFrameChannel — suppress under DEBUG, show under TRACE. */
const isPingPong = (ch: number, msgId: number): boolean =>
  ch === CH.CONTROL && (msgId === CTRL_MSG.PING_REQUEST || msgId === CTRL_MSG.PING_RESPONSE)

// ── Session state machine ─────────────────────────────────────────────────────
const enum State {
  INIT,
  VERSION,
  TLS_HANDSHAKE,
  AUTH,
  SERVICE_DISCOVERY,
  CHANNEL_SETUP,
  RUNNING,
  CLOSED
}

export interface SessionConfig {
  // HU label in SDR
  huName?: string
  // AA tier the phone encodes into (800×480 / 1280×720 / 1920×1080 / 2560×1440 / 3840×2160)
  videoWidth?: number
  videoHeight?: number
  videoDpi?: number
  videoFps?: 30 | 60
  pixelAspectRatioE4?: number
  // Physical HU display
  displayWidth?: number
  displayHeight?: number
  mainSafeAreaTop?: number
  mainSafeAreaBottom?: number
  mainSafeAreaLeft?: number
  mainSafeAreaRight?: number
  // Driver seat position (LHD=0 / RHD=1)
  driverPosition?: 0 | 1
  // BT adapter MAC for BT channel
  btMacAddress?: string
  // WiFi AP BSSID/SSID/password/channel
  wifiBssid?: string
  wifiSsid?: string
  wifiPassword?: string
  wifiChannel?: number
  // FuelType (UNLEADED=1, DIESEL_2=4, ELECTRIC=10, …)
  fuelTypes?: number[]
  evConnectorTypes?: number[]
  // Renderer WebCodecs probe results — only codecs flagged true are advertised
  hevcSupported?: boolean
  vp9Supported?: boolean
  av1Supported?: boolean
  initialNightMode?: boolean
  // When true the secondary (CLUSTER) video sink is advertised in the SDR
  clusterEnabled?: boolean
  clusterWidth: number
  clusterHeight: number
  clusterTierWidth?: number
  clusterTierHeight?: number
  clusterPixelAspectRatioE4?: number
  clusterFps: number
  clusterDpi: number
  clusterSafeAreaTop?: number
  clusterSafeAreaBottom?: number
  clusterSafeAreaLeft?: number
  clusterSafeAreaRight?: number
  disableAudioOutput?: boolean
}

export type VideoCodec = 'h264' | 'h265' | 'vp9' | 'av1'

export class Session extends EventEmitter {
  // Events: 'video-frame', 'video-codec', 'audio-frame', 'audio-start', 'audio-stop',
  //         'mic-start', 'mic-stop',
  //         'host-ui-requested', 'media-metadata', 'media-status',
  //         'connected', 'disconnected', 'error'

  private _state: State = State.INIT
  private _rawParser = new FrameParser()
  private _tls: SessionTls | null = null
  private _pingTimer: ReturnType<typeof setInterval> | null = null
  private _lastPongAt = 0
  private static readonly PING_TIMEOUT_MS = 5_000
  private _proto!: ProtoTypes
  private _control!: ControlChannel
  private _video!: VideoChannel
  private _cluster!: VideoChannel
  private _audio = new Map<number, AudioChannel>()
  private _input!: InputChannel
  private _media!: MediaInfoChannel
  private _mic!: MicChannel
  private _nav!: NavigationChannel
  private _channelMap = new Map<number, number>() // channelId → service type
  private _videoCodecByIndex: VideoCodec[] = []
  private _videoCodec: VideoCodec | null = null
  private _phoneCodecLogged = false
  private _clusterCodecByIndex: VideoCodec[] = []
  private _clusterCodec: VideoCodec | null = null

  constructor(
    private readonly _sock: net.Socket,
    private readonly _cfg: SessionConfig
  ) {
    super()
    this._setupRawPipeline()
  }

  close(reason = 'manual close'): void {
    try {
      this._sock.destroy()
    } catch {
      /* already destroyed */
    }
    if (this._state !== State.CLOSED) {
      this._transition(State.CLOSED, reason)
    }
  }

  // ── Internal wiring ───────────────────────────────────────────────────────

  private _setupRawPipeline(): void {
    // Kernel-level safety net for sudden phone disappearances (battery yank,
    // hard reboot, Wi-Fi crash). With keepalive on, Linux probes the peer
    // after 5 s of idle and tears the socket down with ETIMEDOUT after a
    // few unanswered probes. Without this, the half-open TCP zombie can sit
    // around for ~2 hours (default tcp_keepalive_time = 7200 s).
    try {
      this._sock.setKeepAlive(true, 5_000)
    } catch (e) {
      console.warn('[Session] setKeepAlive failed (ignored)', e)
    }

    this._sock.on('data', (chunk: Buffer) => {
      if (TRACE) {
        const fullDump = this._state <= State.TLS_HANDSHAKE
        const hexPreview =
          fullDump || chunk.length <= 48
            ? chunk.toString('hex')
            : chunk.subarray(0, 48).toString('hex') + `…(+${chunk.length - 48}B)`
        console.log(`[Session] sock← ${chunk.length}B state=${this._state}: ${hexPreview}`)
      }
      if (this._state <= State.TLS_HANDSHAKE) {
        this._rawParser.push(chunk)
      } else {
        this._stripHeaderAndInjectTls(chunk)
      }
    })

    this._sock.on('close', () => this._transition(State.CLOSED, 'socket closed'))

    this._sock.on('end', () => {
      if (this._pingTimer) {
        clearInterval(this._pingTimer)
        this._pingTimer = null
      }
      if (this._state === State.RUNNING) {
        if (DEBUG) {
          console.log(`[Session] phone sent TCP FIN in RUNNING state — keeping write side open`)
        }
      } else {
        if (DEBUG) {
          const stateNames = [
            'INIT',
            'VERSION',
            'TLS_HANDSHAKE',
            'AUTH',
            'SERVICE_DISCOVERY',
            'CHANNEL_SETUP',
            'RUNNING',
            'CLOSED'
          ]
          const stateName = stateNames[this._state] ?? this._state.toString()
          console.log(`[Session] phone sent TCP FIN state=${stateName} — completing close`)
        }
        this._sock.end()
      }
    })

    this._sock.on('error', (err) => {
      this.emit('error', err)
      this._transition(State.CLOSED, err.message)
    })

    this._rawParser.onFrame((frame) => this._handleRawFrame(frame))
  }

  private _tlsBuf = Buffer.allocUnsafe(0)

  private _stripHeaderAndInjectTls(chunk: Buffer): void {
    this._tlsBuf = Buffer.concat([this._tlsBuf, chunk])

    while (this._tlsBuf.length >= 4) {
      // AA frame header
      // -  SHORT (4B):                [ch][flags][size:2BE]
      // -  EXTENDED (8B, FIRST-only): [ch][flags][size:2BE][totalSize:4BE]
      const channelId = this._tlsBuf.readUInt8(0)
      const flags = this._tlsBuf.readUInt8(1)
      const isEncrypted = (flags & 0x08) !== 0
      const isFirst = (flags & 0x01) !== 0
      const isLast = (flags & 0x02) !== 0
      const isExtended = isFirst && !isLast
      const headerLen = isExtended ? 8 : 4

      if (this._tlsBuf.length < headerLen) break

      const payloadSize = this._tlsBuf.readUInt16BE(2)
      const totalLen = headerLen + payloadSize
      if (this._tlsBuf.length < totalLen) break

      const rawPayload = Buffer.from(this._tlsBuf.subarray(headerLen, totalLen))
      this._tlsBuf = this._tlsBuf.subarray(totalLen)

      if (!isEncrypted) {
        if (rawPayload.length < 2) {
          if (DEBUG) console.warn('[Session] post-TLS plaintext too short')
          continue
        }
        const msgId = rawPayload.readUInt16BE(0)
        const payload = rawPayload.subarray(2)
        if (DEBUG) {
          console.log(
            `[Session] ← PLAIN ch=${channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} len=${payload.length}`
          )
        }
        this._handleDecryptedMessage(channelId, flags, msgId, payload)
        continue
      }

      // Encrypted: rawPayload is one full TLS-1.2 record
      if (TRACE) {
        console.log(
          `[Session] TLS inject ch=${channelId} flags=0x${flags.toString(16)} record=${payloadSize}B`
        )
      }
      this._tls?.injectEncrypted(channelId, flags, rawPayload)
    }
  }

  // ── Pre-TLS frame handling ────────────────────────────────────────────────

  private async _handleRawFrame(frame: RawFrame): Promise<void> {
    const { msgId, payload } = frame

    switch (msgId) {
      case CTRL_MSG.VERSION_RESPONSE:
        await this._onVersionResponse(payload)
        break

      case CTRL_MSG.SSL_HANDSHAKE:
        // Feed TLS handshake bytes into the TLS engine
        if (DEBUG) console.log(`[Session] TLS ← phone: ${payload.length} bytes (SSL_HANDSHAKE)`)
        this._tls?.injectHandshakeBytes(payload)
        break

      default:
        // Encrypted frame piggy-backed on the same TCP segment as TLS Finished
        if (this._tls && (frame.flags & 0x08) !== 0) {
          if (DEBUG) {
            console.log(
              `[Session] pre-TLS encrypted frame ch=${frame.channelId} flags=0x${frame.flags.toString(16)} — routing to TLS`
            )
          }
          this._tls.injectEncrypted(frame.channelId, frame.flags, frame.rawPayload)
        } else {
          if (DEBUG) {
            console.log(
              `[Session] pre-TLS unknown msgId=0x${msgId.toString(16)} flags=0x${frame.flags.toString(16)}`
            )
          }
        }
    }
  }

  // ── Post-TLS frame handling ───────────────────────────────────────────────

  private _handleDecryptedMessage(
    channelId: number,
    flags: number,
    msgId: number,
    payload: Buffer
  ): void {
    if (DEBUG && (TRACE || (!isFrameChannel(channelId) && !isPingPong(channelId, msgId)))) {
      const stateName =
        [
          'INIT',
          'VERSION',
          'TLS_HANDSHAKE',
          'AUTH',
          'SERVICE_DISCOVERY',
          'CHANNEL_SETUP',
          'RUNNING',
          'CLOSED'
        ][this._state] ?? this._state.toString()
      console.log(
        `[Session] MSG ch=${channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} len=${payload.length} state=${stateName}`
      )
    }

    if (channelId === CH.CONTROL) {
      this._control?.handleMessage(msgId, payload)
      return
    }

    if (msgId === CTRL_MSG.CHANNEL_OPEN_REQUEST) {
      if (DEBUG) console.log(`[Session] CHANNEL_OPEN_REQUEST ch=${channelId} → responding OK`)
      const respBuf = encode(this._proto.ChannelOpenResponse, { status: STATUS_OK })
      this._sendEncrypted(
        channelId,
        FRAME_FLAGS.ENC_CONTROL,
        CTRL_MSG.CHANNEL_OPEN_RESPONSE,
        respBuf
      )
      return
    }

    if (channelId === CH.VIDEO) {
      if (msgId === AV_MSG.SETUP_REQUEST) {
        this._handleAVSetupRequest(channelId, payload)
        return
      }
      const rawPayload = Buffer.concat([Buffer.allocUnsafe(2), payload])
      rawPayload.writeUInt16BE(msgId, 0)
      const frame = { channelId, flags, msgId, payload, rawPayload }
      this._video?.handleMessage(msgId, payload, frame)
      return
    }

    if (channelId === CH.CLUSTER_VIDEO) {
      if (msgId === AV_MSG.SETUP_REQUEST) {
        this._handleAVSetupRequest(channelId, payload)
        return
      }
      const rawPayload = Buffer.concat([Buffer.allocUnsafe(2), payload])
      rawPayload.writeUInt16BE(msgId, 0)
      const frame = { channelId, flags, msgId, payload, rawPayload }
      this._cluster?.handleMessage(msgId, payload, frame)
      return
    }

    // Audio channels (media/speech/system) share AV wire shape with video.
    const audioCh = this._audio.get(channelId)
    if (audioCh && msgId !== AV_MSG.SETUP_REQUEST) {
      const rawPayload = Buffer.concat([Buffer.allocUnsafe(2), payload])
      rawPayload.writeUInt16BE(msgId, 0)
      const frame = { channelId, flags, msgId, payload, rawPayload }
      audioCh.handleMessage(msgId, payload, frame)
      return
    }

    if (channelId === CH.SENSOR) {
      if (msgId === 0x8001) {
        // SENSOR_MESSAGE_REQUEST
        this._handleSensorStartRequest(payload)
        return
      }
      if (DEBUG) {
        console.log(
          `[Session] sensor ch=${channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} (unhandled)`
        )
      }
      return
    }

    if (channelId === CH.MEDIA_INFO) {
      this._media?.handleMessage(msgId, payload)
      return
    }

    if (channelId === CH.NAVIGATION) {
      this._nav?.handleMessage(msgId, payload)
      return
    }

    if (channelId === CH.MIC_INPUT) {
      if (msgId === AV_MSG.SETUP_REQUEST) {
        this._handleAVSetupRequest(channelId, payload)
        return
      }
      const rawPayload = Buffer.concat([Buffer.allocUnsafe(2), payload])
      rawPayload.writeUInt16BE(msgId, 0)
      const frame = { channelId, flags, msgId, payload, rawPayload }
      this._mic?.handleMessage(msgId, payload, frame)
      return
    }

    if (channelId === CH.WIFI) {
      if (msgId === 0x8001) {
        // WIFI_CREDENTIALS_REQUEST
        if (DEBUG) console.log('[Session] WifiCredentialsRequest received — sending credentials')
        this._handleWifiCredentialsRequest()
        return
      }
      if (DEBUG) {
        console.log(
          `[Session] wifi ch=${channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} (unhandled)`
        )
      }
      return
    }

    // AV SETUP_REQUEST on audio channels
    if (msgId === AV_MSG.SETUP_REQUEST) {
      this._handleAVSetupRequest(channelId, payload)
      return
    }

    // AV START_INDICATION — phone announces it's about to send media frames.
    if (msgId === AV_MSG.START_INDICATION) {
      const start = decodeStart(payload)
      const sessionId = start?.sessionId ?? -1
      const configIdx = start?.configIndex ?? -1

      if (channelId === CH.VIDEO && configIdx >= 0) {
        const codec = this._videoCodecByIndex[configIdx] ?? 'h264'
        if (codec !== this._videoCodec) {
          this._videoCodec = codec
          this.emit('video-codec', codec)
        }
      }

      if (DEBUG) {
        const label =
          channelId === CH.VIDEO
            ? 'video'
            : channelId === CH.MEDIA_AUDIO ||
                channelId === CH.SPEECH_AUDIO ||
                channelId === CH.SYSTEM_AUDIO
              ? 'audio'
              : `ch${channelId}`
        const codecSuffix =
          channelId === CH.VIDEO && this._videoCodec ? ` codec=${this._videoCodec}` : ''
        console.log(
          `[Session] ${label} START_INDICATION ch=${channelId} sessionId=${sessionId} configIdx=${configIdx}${codecSuffix} — stream starting`
        )
      }
      return
    }

    // Input channel: 0x8002 = INPUT_MESSAGE_KEY_BINDING_REQUEST (phone → HU).
    // Phone sends a list of keycodes it wants the HU to bind for input dispatch.
    if (channelId === CH.INPUT && msgId === 0x8002) {
      if (DEBUG) {
        // KeyBindingRequest body: repeated int32 keycodes = 1 (packed)
        console.log(
          `[Session] INPUT KeyBindingRequest (len=${payload.length}) — replying status=OK`
        )
      }
      // KeyBindingResponse: required int32 status = 1; varint tag 0x08, value 0.
      const respBuf = Buffer.from([0x08, 0x00])
      this._sendEncrypted(CH.INPUT, FRAME_FLAGS.ENC_SIGNAL, 0x8003, respBuf)
      return
    }

    if (DEBUG) {
      console.log(
        `[Session] unhandled ch=${channelId} msgId=0x${msgId.toString(16).padStart(4, '0')}`
      )
    }
  }

  // ── Session startup sequence ──────────────────────────────────────────────

  // Entry point — called once the TCP connection is accepted
  async start(): Promise<void> {
    this._proto = await loadProtos()

    // Initialise channels
    this._control = new ControlChannel(this._proto, (ch, flags, msgId, data) =>
      this._sendAA(ch, flags, msgId, data)
    )

    this._video = new VideoChannel(
      (ch, flags, msgId, data) => this._sendEncrypted(ch, flags, msgId, data),
      CH.VIDEO
    )

    this._video.on('frame', (buf: Buffer, ts: bigint) => this.emit('video-frame', buf, ts))

    // Exit/Home on AA display — keep session alive so phone can re-request focus
    this._video.on('host-ui-requested', () => this.emit('host-ui-requested'))
    // Phone requested PROJECTED focus on the main video sink
    this._video.on('video-focus-projected', () => this.emit('video-focus-projected'))

    // Cluster (secondary) display sink — phone may push a Maps overlay or
    // navigation widget here when display_type=CLUSTER is advertised.
    this._cluster = new VideoChannel(
      (ch, flags, msgId, data) => this._sendEncrypted(ch, flags, msgId, data),
      CH.CLUSTER_VIDEO
    )

    this._cluster.on('frame', (buf: Buffer, ts: bigint) =>
      this.emit('cluster-video-frame', buf, ts)
    )
    // Phone requested PROJECTED focus on the cluster sink
    this._cluster.on('video-focus-projected', () => this.emit('cluster-video-focus-projected'))

    // Audio sinks: media (4), speech/guidance (5), system/notification (6)
    for (const channelId of [CH.MEDIA_AUDIO, CH.SPEECH_AUDIO, CH.SYSTEM_AUDIO]) {
      const audio = new AudioChannel(channelId, (ch, flags, msgId, data) =>
        this._sendEncrypted(ch, flags, msgId, data)
      )
      audio.on('pcm', (buf: Buffer, ts: bigint, channel: AudioChannelType) =>
        this.emit('audio-frame', buf, ts, channel, channelId)
      )
      audio.on('start', (channel: AudioChannelType, chId: number) =>
        this.emit('audio-start', channel, chId)
      )
      audio.on('stop', (channel: AudioChannelType, chId: number) =>
        this.emit('audio-stop', channel, chId)
      )
      this._audio.set(channelId, audio)
    }

    // Input channel — outbound only (HU → Phone)
    this._input = new InputChannel((ch, flags, msgId, data) =>
      this._sendEncrypted(ch, flags, msgId, data)
    )

    // Mic channel — outbound HU→Phone PCM, lifecycle driven by phone OPEN_REQUEST.
    this._mic = new MicChannel(CH.MIC_INPUT, (ch, flags, msgId, data) =>
      this._sendEncrypted(ch, flags, msgId, data)
    )
    this._mic.on('mic-start', (chId: number) => this.emit('mic-start', chId))
    this._mic.on('mic-stop', (chId: number) => this.emit('mic-stop', chId))

    // NowPlaying — forward to driver for MediaData mapping
    this._media = new MediaInfoChannel()
    this._media.on('metadata', (m: MediaPlaybackMetadata) => this.emit('media-metadata', m))
    this._media.on('status', (s: MediaPlaybackStatus) => this.emit('media-status', s))

    // Navigation status (turn-by-turn from Maps) — forward to driver
    this._nav = new NavigationChannel()
    this._nav.on('nav-start', () => this.emit('nav-start'))
    this._nav.on('nav-stop', () => this.emit('nav-stop'))
    this._nav.on('nav-status', (s: NavigationStatusUpdate) => this.emit('nav-status', s))
    this._nav.on('nav-turn', (t: NavigationTurnUpdate) => this.emit('nav-turn', t))
    this._nav.on('nav-distance', (d: NavigationDistanceUpdate) => this.emit('nav-distance', d))
    this._control.on('voice-session', (active: boolean) => this.emit('voice-session', active))
    this._control.on('pong', () => {
      this._lastPongAt = Date.now()
    })

    this._control.on('service-discovery-request', (req: Record<string, unknown>) => {
      if (DEBUG) {
        console.log(`[Session] Phone: ${req['labelText'] ?? '?'} / ${req['deviceName'] ?? '?'}`)
      }
      const sdr = buildServiceDiscoveryResponse(this._cfg, this._proto)
      this._videoCodecByIndex = sdr.videoCodecByIndex
      this._clusterCodecByIndex = sdr.clusterCodecByIndex
      this._phoneCodecLogged = false
      this._sendAA(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.SERVICE_DISCOVERY_RESPONSE, sdr.buf)

      // VideoFocusIndication MUST wait until after AVChannelSetupResponse for video
      // Sending it now triggers AudioFocus RELEASE + FIN.

      // Ping after SDR: sendPing() + schedulePing() every 1500ms
      // Ping uses PLAINTEXT (ControlServiceChannel::sendPingRequest uses PLAIN).
      // The interval also doubles as a keepalive watchdog
      this._lastPongAt = Date.now()
      const sendPing = (): void => {
        if (this._state >= State.CLOSED) return
        if (Date.now() - this._lastPongAt > Session.PING_TIMEOUT_MS) {
          console.log(
            `[Session] PING timeout (${Session.PING_TIMEOUT_MS}ms without PING_RESPONSE) — closing session`
          )
          this._transition(State.CLOSED, 'ping timeout')
          try {
            this._sock.destroy()
          } catch {
            /* ignore */
          }
          return
        }
        const pingBuf = encode(this._proto.PingRequest, { timestamp: Date.now() * 1000 })
        this._sendAA(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.PING_REQUEST, pingBuf)
      }
      sendPing()
      this._pingTimer = setInterval(sendPing, 1500)
      if (DEBUG) console.log('[Session] SDR + Ping sent (1500ms interval)')

      this._openChannels()
    })

    // CHANNEL_OPEN_REQUEST on ch=0
    this._control.on('channel-open-request', (channelId: number) => {
      this._control.sendChannelOpenResponse(channelId, STATUS_OK)
    })

    this._control.on('av-setup-request', (channelId: number, payload: Buffer) => {
      this._handleAVSetupRequest(channelId, payload)
    })

    this._control.on('shutdown', (reason: number) => {
      if (DEBUG) console.log(`[Session] Phone shutdown, reason=${reason}`)
      this._transition(State.CLOSED, `phone shutdown reason=${reason}`)
    })

    // Step 1: send version request
    this._transition(State.VERSION)
    this._sendVersionRequest()

    // Pre-RUNNING watchdog
    setTimeout(() => {
      if (this._state >= State.RUNNING || this._state === State.CLOSED) return
      console.warn(
        `[Session] pre-RUNNING watchdog fired: stuck in state ${this._state} after 5s — aborting to trigger USB recovery`
      )
      const err = new Error(
        `session stalled in pre-RUNNING state — phone-side AA service likely zombie`
      )
      try {
        this.emit('error', err)
      } catch {
        /* ignore */
      }
      this.close('pre-RUNNING watchdog')
    }, 5_000)
  }

  // ── Public outbound API (HU → Phone) ─────────────────────────────────────
  /** Touch event in advertised touchscreen-space pixels. No-op outside RUNNING. */
  sendTouch(action: number, pointers: TouchPointer[], actionIndex = 0): void {
    if (this._state !== State.RUNNING || !this._input) return
    this._input.sendTouch(action, pointers, actionIndex)
  }

  // Push captured mic PCM (s16le, 16 kHz mono) to the phone.
  sendMicPcm(buf: Buffer, ts: bigint = BigInt(Date.now()) * 1_000n): void {
    if (this._state !== State.RUNNING || !this._mic) return
    this._mic.pushPcm(buf, ts)
  }

  // HW button event. Codes in InputChannel.BUTTON_KEY
  sendButton(keyCode: number | readonly number[], down: boolean): void {
    if (this._state !== State.RUNNING || !this._input) return
    this._input.sendButton(keyCode, down)
  }

  // Rotary-encoder delta event (-1 = previous, +1 = next)
  sendRotary(direction: -1 | 1): void {
    if (this._state !== State.RUNNING || !this._input) return
    this._input.sendRotary(direction)
  }

  // ── Sensor pushes (HU → Phone) ─────────────────────────────────────────────
  // All writes go to CH.SENSOR / msgId 0x8003 (SENSOR_MESSAGE_BATCH).
  // SensorBatch field number = SensorType id.

  private _sendSensorBatch(sensorBatchField: number, innerData: Buffer): void {
    if (this._state !== State.RUNNING) return
    const sensorBatch = fieldLenDelim(sensorBatchField, innerData)
    this._sendEncrypted(CH.SENSOR, FRAME_FLAGS.ENC_SIGNAL, 0x8003, sensorBatch)
    if (DEBUG) console.log(`[Session] SensorBatch field=${sensorBatchField} ${innerData.length}B`)
  }

  // level%, range[m], lowFuelWarning
  sendFuelData(level: number, range?: number, lowFuelWarning?: boolean): void {
    const parts: Buffer[] = [fieldVarint(1, level)]
    if (range !== undefined) parts.push(fieldVarint(2, range))
    if (lowFuelWarning !== undefined) parts.push(fieldVarint(3, lowFuelWarning ? 1 : 0))
    this._sendSensorBatch(6, Buffer.concat(parts))
  }

  // speed in mm/s (m/s × 1000)
  sendSpeedData(speedMmS: number, cruiseEngaged?: boolean, cruiseSetSpeedMmS?: number): void {
    const parts: Buffer[] = [fieldVarint(1, speedMmS)]
    if (cruiseEngaged !== undefined) parts.push(fieldVarint(2, cruiseEngaged ? 1 : 0))
    if (cruiseSetSpeedMmS !== undefined) parts.push(fieldVarint(4, cruiseSetSpeedMmS))
    this._sendSensorBatch(3, Buffer.concat(parts))
  }

  // rpm × 1000
  sendRpmData(rpmE3: number): void {
    this._sendSensorBatch(4, fieldVarint(1, rpmE3))
  }

  // Gear enum: NEUTRAL=0, 1..10 manual, DRIVE=100, PARK=101, REVERSE=102
  sendGearData(gear: number): void {
    this._sendSensorBatch(8, fieldVarint(1, gear))
  }

  sendNightModeData(nightMode: boolean): void {
    this._sendSensorBatch(10, fieldVarint(1, nightMode ? 1 : 0))
  }

  sendParkingBrakeData(engaged: boolean): void {
    this._sendSensorBatch(7, fieldVarint(1, engaged ? 1 : 0))
  }

  // headLight: 1=OFF, 2=ON, 3=HIGH. turnIndicator: 1=NONE, 2=LEFT, 3=RIGHT
  sendLightData(headLight?: 1 | 2 | 3, hazardLights?: boolean, turnIndicator?: 1 | 2 | 3): void {
    const parts: Buffer[] = []
    if (headLight !== undefined) parts.push(fieldVarint(1, headLight))
    if (turnIndicator !== undefined) parts.push(fieldVarint(2, turnIndicator))
    if (hazardLights !== undefined) parts.push(fieldVarint(3, hazardLights ? 1 : 0))
    if (parts.length === 0) return
    this._sendSensorBatch(17, Buffer.concat(parts))
  }

  // temperature in m°C, pressure in Pa (kPa × 1000)
  sendEnvironmentData(temperatureE3?: number, pressureE3?: number, rain?: number): void {
    const parts: Buffer[] = []
    if (temperatureE3 !== undefined) parts.push(fieldVarint(1, temperatureE3))
    if (pressureE3 !== undefined) parts.push(fieldVarint(2, pressureE3))
    if (rain !== undefined) parts.push(fieldVarint(3, rain))
    if (parts.length === 0) return
    this._sendSensorBatch(11, Buffer.concat(parts))
  }

  // km × 10
  sendOdometerData(totalKmE1: number, tripKmE1?: number): void {
    const parts: Buffer[] = [fieldVarint(1, totalKmE1)]
    if (tripKmE1 !== undefined) parts.push(fieldVarint(2, tripKmE1))
    this._sendSensorBatch(5, Buffer.concat(parts))
  }

  // Restriction bitmask. UNRESTRICTED=0
  sendDrivingStatusData(status: number): void {
    this._sendSensorBatch(13, fieldVarint(1, status))
  }

  /**
   * GPS / GNSS fix.  SensorBatch field 1 = repeated LocationData.
   *
   * Proto (`LocationData.proto`):
   *   2  latitude_e7   int32   (REQUIRED, degrees × 1e7)
   *   3  longitude_e7  int32   (REQUIRED, degrees × 1e7)
   *   4  accuracy_e3   uint32  (meters × 1000)
   *   5  altitude_e2   int32   (meters × 100)
   *   6  speed_e3      int32   (m/s × 1000)
   *   7  bearing_e6    int32   (degrees × 1e6)
   */
  sendGpsLocationData(opts: {
    latDeg: number
    lngDeg: number
    accuracyM?: number
    altitudeM?: number
    speedMs?: number
    bearingDeg?: number
  }): void {
    const parts: Buffer[] = [
      fieldVarint(2, Math.round(opts.latDeg * 1e7)),
      fieldVarint(3, Math.round(opts.lngDeg * 1e7))
    ]
    if (opts.accuracyM !== undefined) parts.push(fieldVarint(4, Math.round(opts.accuracyM * 1000)))
    if (opts.altitudeM !== undefined) parts.push(fieldVarint(5, Math.round(opts.altitudeM * 100)))
    if (opts.speedMs !== undefined) parts.push(fieldVarint(6, Math.round(opts.speedMs * 1000)))
    if (opts.bearingDeg !== undefined) parts.push(fieldVarint(7, Math.round(opts.bearingDeg * 1e6)))
    this._sendSensorBatch(1, Buffer.concat(parts))
  }

  /**
   * EV battery / energy model. Sent as SensorBatch.vehicle_energy_model_data
   * (field 23) — Maps reads min_usable_capacity.watt_hours as the *current*
   * battery level
   *
   * capacityWh: gross battery capacity (e.g. 50000 = 50 kWh)
   * currentWh:  current battery level in Wh
   * rangeM:     remaining range in metres
   * opts.maxChargePowerW / maxDischargePowerW: defaults to 150 kW each
   */
  sendVehicleEnergyModel(
    capacityWh: number,
    currentWh: number,
    rangeM: number,
    opts: { maxChargePowerW?: number; maxDischargePowerW?: number; auxiliaryWhPerKm?: number } = {}
  ): void {
    if (capacityWh <= 0 || currentWh <= 0 || rangeM <= 0) return

    // EnergyValue { watt_hours = 1 }
    const energyValue = (wh: number): Buffer => fieldVarint(1, wh)

    // BatteryConfig {
    //   config_id=1, min_usable_capacity=3, max_capacity=4,
    //   reserve_energy=8, max_charge_power_w=9, max_discharge_power_w=10,
    //   regen_braking_capable=11
    // }
    const reserve = Math.round(capacityWh * 0.05)
    const maxCharge = opts.maxChargePowerW ?? 150_000
    const maxDischarge = opts.maxDischargePowerW ?? 150_000
    const battery = Buffer.concat([
      fieldVarint(1, 1), // config_id
      fieldLenDelim(3, energyValue(currentWh)), // min_usable_capacity = current level
      fieldLenDelim(4, energyValue(capacityWh)), // max_capacity = gross
      fieldLenDelim(8, energyValue(reserve)), // reserve_energy
      fieldVarint(9, maxCharge),
      fieldVarint(10, maxDischarge),
      fieldVarint(11, 1) // regen_braking_capable = true
    ])

    // EnergyRate { rate=1 (float) }
    // EnergyConsumption { driving=1, auxiliary=2, aerodynamic=3 }
    const whPerKm = (currentWh / rangeM) * 1000
    const aux = opts.auxiliaryWhPerKm ?? 2.0
    const consumption = Buffer.concat([
      fieldLenDelim(1, fieldFloat(1, whPerKm)),
      fieldLenDelim(2, fieldFloat(1, aux)),
      fieldLenDelim(3, fieldFloat(1, 0.36))
    ])

    // ChargingPrefs { mode=3 } — mode 1 = standard
    const chargingPrefs = fieldVarint(3, 1)

    // VehicleEnergyModel { battery=1, consumption=2, charging_prefs=12 }
    const vem = Buffer.concat([
      fieldLenDelim(1, battery),
      fieldLenDelim(2, consumption),
      fieldLenDelim(12, chargingPrefs)
    ])

    this._sendSensorBatch(23, vem)
    if (DEBUG)
      console.log(
        `[Session] SensorBatch: VEM cap=${capacityWh}Wh cur=${currentWh}Wh range=${rangeM}m`
      )
  }

  // Tell the phone we have PROJECTED focus on the main video channel.
  // Sent on AA-UI entry and whenever the renderer's decoder needs a keyframe.
  // Phone resumes streaming (if it had stopped) and emits an IDR when ready.
  requestKeyframe(): void {
    if (this._state !== State.RUNNING) return
    this._sendEncrypted(
      CH.VIDEO,
      FRAME_FLAGS.ENC_SIGNAL,
      AV_MSG.VIDEO_FOCUS_INDICATION,
      Buffer.from([0x08, 0x01])
    )
    if (DEBUG) console.log('[Session] main video focus indication (PROJECTED) sent')
  }

  // Tell the phone we have PROJECTED focus on the cluster channel. Same
  // shape as main: a single VideoFocusIndication. Repeated calls are
  // idempotent and don't interrupt the running stream.
  requestClusterKeyframe(): void {
    if (this._state !== State.RUNNING) return
    this._sendEncrypted(
      CH.CLUSTER_VIDEO,
      FRAME_FLAGS.ENC_SIGNAL,
      AV_MSG.VIDEO_FOCUS_INDICATION,
      Buffer.from([0x08, 0x01])
    )
    if (DEBUG) console.log('[Session] cluster video focus indication (PROJECTED) sent')
  }

  // HU-initiated shutdown via ByeByeRequest
  async requestShutdown(reason = 1 /* USER_SELECTION */): Promise<void> {
    if (this._state >= State.CLOSED) return
    const payload = Buffer.from([0x08, reason & 0xff])
    if (DEBUG) console.log(`[Session] requesting shutdown reason=${reason}`)
    try {
      this._sendEncrypted(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.SHUTDOWN_REQUEST, payload)
    } catch (err) {
      if (DEBUG) console.warn(`[Session] shutdown send failed: ${(err as Error).message}`)
    }
    // Wait for the encrypted ByeBye to actually leave the TLS stack and hit
    // the underlying socket buffer
    let writeTimer: NodeJS.Timeout | null = null
    try {
      await Promise.race([
        this._tls?.drain() ?? Promise.resolve(),
        new Promise<void>((resolve) => {
          writeTimer = setTimeout(resolve, 500)
        })
      ])
    } finally {
      if (writeTimer) clearTimeout(writeTimer)
    }
    this._transition(State.CLOSED, 'hu-initiated shutdown')
    try {
      this._sock.end()
    } catch {
      /* ignore */
    }
  }

  private _sendVersionRequest(): void {
    // VERSION_REQUEST: major(2BE) + minor(2BE)
    const data = Buffer.allocUnsafe(4)
    data.writeUInt16BE(VERSION.MAJOR, 0)
    data.writeUInt16BE(VERSION.MINOR, 2)
    const frame = encodeFrame(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.VERSION_REQUEST, data)
    this._sock.write(frame)
  }

  private async _onVersionResponse(payload: Buffer): Promise<void> {
    // payload: [major(2BE)][minor(2BE)][status(2BE)]
    if (payload.length < 6) {
      if (DEBUG) console.error('[Session] VERSION_RESPONSE too short')
      return
    }
    const major = payload.readUInt16BE(0)
    const minor = payload.readUInt16BE(2)
    const status = payload.readUInt16BE(4)

    if (status === VERSION.STATUS_MISMATCH) {
      this._transition(State.CLOSED, `version mismatch ${major}.${minor}`)
      return
    }
    if (DEBUG) console.log(`[Session] Version negotiated: ${major}.${minor}`)

    // Step 2: start TLS handshake
    this._transition(State.TLS_HANDSHAKE)
    await this._startTls()
  }

  private async _startTls(): Promise<void> {
    this._tls = new SessionTls({
      writeRaw: (frame) => this._sock.write(frame),
      onDecryptedMessage: (ch, fl, mid, p) => this._handleDecryptedMessage(ch, fl, mid, p),
      onSecureConnect: () => {
        this._transition(State.AUTH)
        void this._postTlsSetup()
      },
      onError: (err) => this.emit('error', err),
      isHandshakePhase: () => this._state === State.TLS_HANDSHAKE
    })
  }

  private async _postTlsSetup(): Promise<void> {
    // AUTH_COMPLETE is sent PLAINTEXT
    const authBuf = encode(this._proto.AuthCompleteIndication, { status: STATUS_OK })
    if (DEBUG) console.log(`[Session] AUTH_COMPLETE proto bytes: ${authBuf.toString('hex')}`)
    this._sendAA(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.AUTH_COMPLETE, authBuf)
    this._transition(State.SERVICE_DISCOVERY)
    if (DEBUG) console.log('[Session] AUTH_COMPLETE sent — waiting for SERVICE_DISCOVERY_REQUEST')
  }

  // ── Channel open sequence ─────────────────────────────────────────────────

  private _openChannels(): void {
    // Phone sends CHANNEL_OPEN_REQUEST on each service channel; we respond on
    // the same channel. HU never initiates channel open.
    this._transition(State.CHANNEL_SETUP)
    if (DEBUG) {
      console.log(
        '[Session] Channel setup — waiting for phone CHANNEL_OPEN_REQUEST on each service channel'
      )
    }
  }

  // ── AV channel setup ──────────────────────────────────────────────────────

  private _handleAVSetupRequest(channelId: number, payload: Buffer): void {
    const req = decode(this._proto.AVChannelSetupRequest, payload)
    const codec = req['mediaCodecType'] as number
    if (DEBUG) console.log(`[Session] AVSetupRequest ch=${channelId} codec=${codec}`)

    // Push advertised rate/channels into AudioChannel so it labels 'pcm' emits.
    const audioCh = this._audio.get(channelId)
    if (audioCh) {
      const cfg =
        channelId === CH.MEDIA_AUDIO
          ? { rate: 48000, ch: 2 }
          : channelId === CH.SPEECH_AUDIO
            ? { rate: 16000, ch: 1 }
            : { rate: 16000, ch: 1 } // SYSTEM_AUDIO
      audioCh.handleSetupRequest(codec, cfg.rate, cfg.ch)
    } else if (channelId === CH.MIC_INPUT && this._mic) {
      // Mic uses the same SETUP_REQUEST/RESPONSE flow but is outbound;
      // the format we advertised is 16 kHz mono.
      this._mic.handleSetupRequest(codec, 16000, 1)
    }

    // mediaStatus MUST be OK(2) — NONE(0) is treated as FAIL and drops the session.
    let configIdx = 0
    if (channelId === CH.VIDEO) {
      const want: VideoCodec =
        codec === MEDIA_CODEC.VIDEO_H265
          ? 'h265'
          : codec === MEDIA_CODEC.VIDEO_VP9
            ? 'vp9'
            : codec === MEDIA_CODEC.VIDEO_AV1
              ? 'av1'
              : 'h264'
      const idx = this._videoCodecByIndex.indexOf(want)
      if (idx >= 0) configIdx = idx
      if (this._videoCodec !== want) {
        this._videoCodec = want
        this.emit('video-codec', want)
        if (DEBUG) console.log(`[Session] video codec selected: ${want} (configIdx=${configIdx})`)
      }
      if (!this._phoneCodecLogged) {
        this._phoneCodecLogged = true
        console.log(
          `[Session] ★ phone picked video codec: ${want.toUpperCase()} ` +
            `(offered: ${this._videoCodecByIndex.join(', ')})`
        )
      }
    } else if (channelId === CH.CLUSTER_VIDEO) {
      const want: VideoCodec =
        codec === MEDIA_CODEC.VIDEO_H265
          ? 'h265'
          : codec === MEDIA_CODEC.VIDEO_VP9
            ? 'vp9'
            : codec === MEDIA_CODEC.VIDEO_AV1
              ? 'av1'
              : 'h264'
      const idx = this._clusterCodecByIndex.indexOf(want)
      if (idx >= 0) configIdx = idx
      if (this._clusterCodec !== want) {
        this._clusterCodec = want
        this.emit('cluster-video-codec', want)
        if (DEBUG) {
          console.log(`[Session] cluster codec selected: ${want} (configIdx=${configIdx})`)
        }
      }
    }
    const respBuf = encode(this._proto.AVChannelSetupResponse, {
      mediaStatus: AV_SETUP_STATUS.OK,
      maxUnacked: 1,
      configs: [configIdx]
    })
    this._sendEncrypted(channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.SETUP_RESPONSE, respBuf)
    if (DEBUG) {
      console.log(
        `[Session] AVChannelSetupResponse ch=${channelId} status=OK(${AV_SETUP_STATUS.OK}) sent`
      )
    }

    if (channelId === CH.VIDEO || channelId === CH.CLUSTER_VIDEO) {
      // VideoFocusIndication(PROJECTED, unsolicited=false) keyframe-request
      this._sendEncrypted(
        channelId,
        FRAME_FLAGS.ENC_SIGNAL,
        AV_MSG.VIDEO_FOCUS_INDICATION,
        Buffer.from([0x08, 0x01])
      )
      if (DEBUG) {
        const label = channelId === CH.CLUSTER_VIDEO ? 'cluster' : 'main'
        console.log(`[Session] VideoFocusIndication ${label} (PROJECTED, unsolicited=false) sent`)
      }

      if (channelId === CH.VIDEO) {
        // No AVChannelStartIndication — phone sends START_INDICATION when ready
        this._transition(State.RUNNING)
        this.emit('connected')
        if (DEBUG)
          console.log(
            `[Session] Video channel ready — waiting for ${this._videoCodec ?? 'h264'} frames from phone`
          )
      }
    }
  }

  // ── Sensor channel ────────────────────────────────────────────────────────

  private _handleSensorStartRequest(payload: Buffer): void {
    // SensorRequest: field 1 (varint) = SensorType
    let sensorType = 0
    if (payload.length >= 2 && payload[0] === 0x08) {
      sensorType = payload[1]!
    }
    if (DEBUG) console.log(`[Session] SensorStartRequest type=${sensorType}`)

    // SensorStartResponse: status=SUCCESS(0). msgId 0x8002 = SENSOR_MESSAGE_RESPONSE.
    this._sendEncrypted(CH.SENSOR, FRAME_FLAGS.ENC_SIGNAL, 0x8002, Buffer.from([0x08, 0x00]))

    // SensorBatch (msgId 0x8003) — emit initial value per type.
    if (sensorType === 13) {
      // DrivingStatus = UNRESTRICTED(0)
      this._sendEncrypted(
        CH.SENSOR,
        FRAME_FLAGS.ENC_SIGNAL,
        0x8003,
        Buffer.from([0x6a, 0x02, 0x08, 0x00])
      )
      if (DEBUG) console.log('[Session] SensorBatch: DrivingStatus=UNRESTRICTED sent')
    } else if (sensorType === 10) {
      const initial = this._cfg.initialNightMode === true
      this._sendEncrypted(
        CH.SENSOR,
        FRAME_FLAGS.ENC_SIGNAL,
        0x8003,
        Buffer.from([0x52, 0x02, 0x08, initial ? 0x01 : 0x00])
      )
      if (DEBUG) console.log(`[Session] SensorBatch: NightMode=${initial} sent`)
    }
    // No-batch sensor types (most of them) emit ack-only — silent under DEBUG.
  }

  // ── WiFi Projection channel (ch=14) ──────────────────────────────────────

  private _handleWifiCredentialsRequest(): void {
    // WifiCredentialsResponse (msgId 0x8002) on the WiFi projection channel:
    //   f1 = car_wifi_password (string)
    //   f2 = car_wifi_security_mode (varint, WPA2_PERSONAL = 5 in the new
    //        aap_protobuf WifiSecurityMode enum used by this message;
    //        distinct from the legacy aasdk_proto SecurityMode enum where
    //        WPA2_PERSONAL = 8 used by the RFCOMM-side WifiInfoResponse)
    //   f3 = car_wifi_ssid (string)
    //   f5 = access_point_type = STATIC (0)
    const ssid = this._cfg.wifiSsid ?? ''
    const pass = this._cfg.wifiPassword ?? ''

    if (!ssid) {
      if (DEBUG) {
        console.warn(
          '[Session] WifiCredentialsRequest: no wifiSsid configured — sending empty response'
        )
      }
    }

    const parts: Buffer[] = []

    if (pass.length > 0) {
      const passBytes = Buffer.from(pass, 'utf-8')
      parts.push(Buffer.from([0x0a]))
      parts.push(_encodeVarint(passBytes.length))
      parts.push(passBytes)
    }

    parts.push(Buffer.from([0x10, 0x05])) // security_mode = WPA2_PERSONAL

    if (ssid.length > 0) {
      const ssidBytes = Buffer.from(ssid, 'utf-8')
      parts.push(Buffer.from([0x1a]))
      parts.push(_encodeVarint(ssidBytes.length))
      parts.push(ssidBytes)
    }

    parts.push(Buffer.from([0x28, 0x00])) // access_point_type = STATIC

    const respBuf = Buffer.concat(parts)
    if (DEBUG) {
      console.log(
        `[Session] WifiCredentialsResponse: ssid="${ssid}" security=WPA2_PERSONAL(5) type=STATIC`
      )
    }
    this._sendEncrypted(CH.WIFI, FRAME_FLAGS.ENC_SIGNAL, 0x8002, respBuf)
  }

  // ── Frame sending ─────────────────────────────────────────────────────────

  // Send an AA frame. Encrypted (flags & 0x08) → TLS via tlsSocket
  private _sendAA(channelId: number, flags: number, msgId: number, data: Buffer): void {
    const isEncrypted = (flags & 0x08) !== 0

    if (!isEncrypted) {
      const frame = encodeFrame(channelId, flags, msgId, data)
      if (DEBUG && (TRACE || !isPingPong(channelId, msgId))) {
        console.log(
          `[Session] sock→ PLAIN ch=${channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} ${frame.length}B`
        )
      }
      this._sock.write(frame)
      return
    }

    if (!this._tls || this._state < State.AUTH) {
      if (DEBUG) console.warn('[Session] _sendAA: TLS not ready for encrypted frame')
      return
    }

    const msgIdBuf = Buffer.allocUnsafe(2)
    msgIdBuf.writeUInt16BE(msgId, 0)
    const cleartext = Buffer.concat([msgIdBuf, data])
    this._tls.sendEncrypted(channelId, flags, cleartext)
  }

  private _sendEncrypted(channelId: number, flags: number, msgId: number, data: Buffer): void {
    this._sendAA(channelId, flags, msgId, data)
  }

  // ── State machine ─────────────────────────────────────────────────────────

  private _transition(newState: State, reason?: string): void {
    this._state = newState
    if (newState === State.CLOSED) {
      if (this._pingTimer) {
        clearInterval(this._pingTimer)
        this._pingTimer = null
      }
      // Don't destroy the socket — phone controls lifetime; just notify.
      this.emit('disconnected', reason)
    }
  }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/** Encode a non-negative integer as a protobuf varint. */
function _encodeVarint(value: number): Buffer {
  const bytes: number[] = []
  while (value > 0x7f) {
    bytes.push((value & 0x7f) | 0x80)
    value >>>= 7
  }
  bytes.push(value & 0x7f)
  return Buffer.from(bytes)
}
