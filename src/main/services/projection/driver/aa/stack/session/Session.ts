/**
 * AA wireless session — one per TCP connection.
 * State: INIT → VERSION → TLS_HANDSHAKE → AUTH → SERVICE_DISCOVERY
 *        → CHANNEL_SETUP → RUNNING → CLOSED
 */

import { EventEmitter } from 'node:events'
import * as net from 'node:net'
import * as tls from 'node:tls'
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
import { fieldFloat, fieldLenDelim, fieldVarint } from '../channels/protoEnc.js'
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
import { HU_CERT_PEM, HU_KEY_PEM } from '../crypto/cert.js'
import { createTlsClient, TlsBridge } from '../crypto/TlsBridge.js'
import { encodeFrame, FrameParser, type RawFrame } from '../frame/codec.js'
import { decode, encode, loadProtos, type ProtoTypes } from '../proto/index.js'
import { ControlChannel } from './ControlChannel.js'

/** Per-frame chatter is suppressed for these channels under DEBUG=1.
 *  Set TRACE=1 to see them anyway. */
const isFrameChannel = (ch: number): boolean =>
  ch === CH.VIDEO ||
  ch === CH.MEDIA_AUDIO ||
  ch === CH.SPEECH_AUDIO ||
  ch === CH.SYSTEM_AUDIO ||
  ch === CH.INPUT ||
  ch === CH.MIC_INPUT

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
  /** HU label in SDR */
  huName?: string
  /** AA tier the phone encodes into (800×480 / 1280×720 / 1920×1080 / 2560×1440 / 3840×2160) */
  videoWidth?: number
  videoHeight?: number
  videoDpi?: number
  videoFps?: 30 | 60
  /** Physical HU display — drives margin / inset computation for non-tier ARs */
  displayWidth?: number
  displayHeight?: number
  /** Driver seat position. Matches LIVI HandDriveType (LHD=0 / RHD=1). */
  driverPosition?: 0 | 1
  /** BT adapter MAC for BT channel */
  btMacAddress?: string
  /** WiFi AP BSSID/SSID/password/channel */
  wifiBssid?: string
  wifiSsid?: string
  wifiPassword?: string
  wifiChannel?: number
  /** FuelType enum values from aap_protobuf (UNLEADED=1, DIESEL_2=4, ELECTRIC=10, …).*/
  fuelTypes?: number[]
  evConnectorTypes?: number[]
}

export class Session extends EventEmitter {
  // Events: 'video-frame', 'audio-frame', 'audio-start', 'audio-stop',
  //         'mic-start', 'mic-stop',
  //         'host-ui-requested', 'media-metadata', 'media-status',
  //         'connected', 'disconnected', 'error'

  private _state: State = State.INIT
  private _rawParser = new FrameParser()
  private _bridge!: TlsBridge
  private _tlsSocket!: tls.TLSSocket
  private _pingTimer: ReturnType<typeof setInterval> | null = null
  // (channelId, flags) for the next TLS record
  private _pendingChannelId = 0
  private _pendingFlags = 0
  private _writeChain: Promise<void> = Promise.resolve()
  // (channelId, flags) per injected TLS record, consumed in 'data' order
  private _channelQueue: Array<{ channelId: number; flags: number }> = []
  private _proto!: ProtoTypes
  private _control!: ControlChannel
  private _video!: VideoChannel
  private _audio = new Map<number, AudioChannel>()
  private _input!: InputChannel
  private _media!: MediaInfoChannel
  private _mic!: MicChannel
  private _nav!: NavigationChannel
  private _channelMap = new Map<number, number>() // channelId → service type

  constructor(
    private readonly _sock: net.Socket,
    private readonly _cfg: SessionConfig
  ) {
    super()
    this._setupRawPipeline()
  }

  // ── Internal wiring ───────────────────────────────────────────────────────

  private _setupRawPipeline(): void {
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

  /**
   * Per-channel cleartext reassembly for multi-fragment encrypted messages.
   */
  private _tlsCleartextFragments = new Map<number, { parts: Buffer[]; flags: number }>()

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

      // Encrypted: rawPayload is one full TLS-1.2 record. Pure wire-level
      // detail — only useful for protocol bring-up, hidden behind TRACE.
      if (TRACE) {
        console.log(
          `[Session] TLS inject ch=${channelId} flags=0x${flags.toString(16)} record=${payloadSize}B`
        )
      }
      this._channelQueue.push({ channelId, flags })
      this._bridge.injectBytes(rawPayload)
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
        if (this._bridge) this._bridge.injectBytes(payload)
        break

      default:
        // Encrypted frame piggy-backed on the same TCP segment as TLS Finished
        // (e.g. AUTH_COMPLETE immediately following TLS handshake).
        if (this._bridge && this._tlsSocket && (frame.flags & 0x08) !== 0) {
          if (DEBUG) {
            console.log(
              `[Session] pre-TLS encrypted frame ch=${frame.channelId} flags=0x${frame.flags.toString(16)} — routing to TLS`
            )
          }
          this._channelQueue.push({ channelId: frame.channelId, flags: frame.flags })
          this._bridge.injectBytes(frame.rawPayload)
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

    // CHANNEL_OPEN_REQUEST normally arrives on the target service channel,
    // not ch=0 — aasdk routes by frame channelId; only session-level control
    // (Version/SDR/Ping/AudioFocus/Shutdown) goes on ch=0.
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
      let sessionId = -1
      let configIdx = -1
      // Manual decode: field 1 tag=0x08, field 2 tag=0x10
      let i = 0
      while (i < payload.length) {
        const tag = payload[i++]!
        if (tag === 0x08 && i < payload.length) {
          sessionId = payload[i++]!
        } else if (tag === 0x10 && i < payload.length) {
          configIdx = payload[i++]!
        } else break
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
        console.log(
          `[Session] ${label} START_INDICATION ch=${channelId} sessionId=${sessionId} configIdx=${configIdx} — stream starting`
        )
      }
      return
    }

    // Input channel: 0x8002 = INPUT_MESSAGE_KEY_BINDING_REQUEST (phone → HU).
    // Phone sends a list of keycodes it wants the HU to bind for input dispatch.
    // We MUST reply with INPUT_MESSAGE_KEY_BINDING_RESPONSE (0x8003, status=0)
    // — without that ACK the phone disables key delivery on the input channel,
    // which is why arrows / NAVIGATE_* don't reach the focused view.
    if (channelId === CH.INPUT && msgId === 0x8002) {
      if (DEBUG) {
        // KeyBindingRequest body: repeated int32 keycodes = 1 (packed). We only
        // need to log it — the contract is to ACK regardless of contents.
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

  /** Entry point — called once the TCP connection is accepted. */
  async start(): Promise<void> {
    this._proto = await loadProtos()

    // Initialise channels
    this._control = new ControlChannel(this._proto, (ch, flags, msgId, data) =>
      this._sendAA(ch, flags, msgId, data)
    )

    this._video = new VideoChannel((ch, flags, msgId, data) =>
      this._sendEncrypted(ch, flags, msgId, data)
    )

    this._video.on('frame', (buf: Buffer, ts: bigint) => this.emit('video-frame', buf, ts))

    // Exit/Home on AA display — keep session alive so phone can re-request focus.
    this._video.on('host-ui-requested', () => this.emit('host-ui-requested'))

    // Audio sinks: media (4), speech/guidance (5), system/notification (6).
    // 'audio-start'/'audio-stop' drive ProjectionAudio's `mediaActive` gate.
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

    this._control.on('service-discovery-request', (req: Record<string, unknown>) => {
      if (DEBUG) {
        console.log(`[Session] Phone: ${req['labelText'] ?? '?'} / ${req['deviceName'] ?? '?'}`)
      }
      const sdResp = this._buildServiceDiscoveryResponse()
      this._sendAA(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.SERVICE_DISCOVERY_RESPONSE, sdResp)

      // VideoFocusIndication MUST wait until after AVChannelSetupResponse for video
      // Sending it now triggers AudioFocus RELEASE + FIN.

      // Ping after SDR: sendPing() + schedulePing() every 1500ms
      // Ping uses PLAINTEXT (ControlServiceChannel::sendPingRequest uses PLAIN).
      const sendPing = (): void => {
        if (this._state >= State.CLOSED) return
        const pingBuf = encode(this._proto.PingRequest, { timestamp: Date.now() * 1000 })
        this._sendAA(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.PING_REQUEST, pingBuf)
      }
      sendPing()
      this._pingTimer = setInterval(sendPing, 1500)
      if (DEBUG) console.log('[Session] SDR + Ping sent (1500ms interval)')

      this._openChannels()
    })

    // CHANNEL_OPEN_REQUEST on ch=0 (rare — normally arrives on the target channel)
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
  }

  // ── Public outbound API (HU → Phone) ─────────────────────────────────────
  /** Touch event in advertised touchscreen-space pixels. No-op outside RUNNING. */
  sendTouch(action: number, pointers: TouchPointer[], actionIndex = 0): void {
    if (this._state !== State.RUNNING || !this._input) return
    this._input.sendTouch(action, pointers, actionIndex)
  }

  /**
   * Push captured mic PCM (s16le, 16 kHz mono) to the phone.
   * No-op outside RUNNING or when the mic channel hasn't been opened by the
   * phone — the MicChannel itself drops frames silently in those cases.
   */
  sendMicPcm(buf: Buffer, ts: bigint = BigInt(Date.now()) * 1_000n): void {
    if (this._state !== State.RUNNING || !this._mic) return
    this._mic.pushPcm(buf, ts)
  }

  /** HW button event. Codes in InputChannel.BUTTON_KEY. */
  sendButton(keyCode: number | readonly number[], down: boolean): void {
    if (this._state !== State.RUNNING || !this._input) return
    this._input.sendButton(keyCode, down)
  }

  /** Rotary-encoder delta event (-1 = previous, +1 = next). For in-list scroll. */
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

  /** level%, range[m], lowFuelWarning. */
  sendFuelData(level: number, range?: number, lowFuelWarning?: boolean): void {
    const parts: Buffer[] = [fieldVarint(1, level)]
    if (range !== undefined) parts.push(fieldVarint(2, range))
    if (lowFuelWarning !== undefined) parts.push(fieldVarint(3, lowFuelWarning ? 1 : 0))
    this._sendSensorBatch(6, Buffer.concat(parts))
  }

  /** speed in mm/s (m/s × 1000). */
  sendSpeedData(speedMmS: number, cruiseEngaged?: boolean, cruiseSetSpeedMmS?: number): void {
    const parts: Buffer[] = [fieldVarint(1, speedMmS)]
    if (cruiseEngaged !== undefined) parts.push(fieldVarint(2, cruiseEngaged ? 1 : 0))
    if (cruiseSetSpeedMmS !== undefined) parts.push(fieldVarint(4, cruiseSetSpeedMmS))
    this._sendSensorBatch(3, Buffer.concat(parts))
  }

  /** rpm × 1000. */
  sendRpmData(rpmE3: number): void {
    this._sendSensorBatch(4, fieldVarint(1, rpmE3))
  }

  /** Gear enum: NEUTRAL=0, 1..10 manual, DRIVE=100, PARK=101, REVERSE=102. */
  sendGearData(gear: number): void {
    this._sendSensorBatch(8, fieldVarint(1, gear))
  }

  sendNightModeData(nightMode: boolean): void {
    this._sendSensorBatch(10, fieldVarint(1, nightMode ? 1 : 0))
  }

  sendParkingBrakeData(engaged: boolean): void {
    this._sendSensorBatch(7, fieldVarint(1, engaged ? 1 : 0))
  }

  /** headLight: 1=OFF, 2=ON, 3=HIGH. turnIndicator: 1=NONE, 2=LEFT, 3=RIGHT. */
  sendLightData(headLight?: 1 | 2 | 3, hazardLights?: boolean, turnIndicator?: 1 | 2 | 3): void {
    const parts: Buffer[] = []
    if (headLight !== undefined) parts.push(fieldVarint(1, headLight))
    if (turnIndicator !== undefined) parts.push(fieldVarint(2, turnIndicator))
    if (hazardLights !== undefined) parts.push(fieldVarint(3, hazardLights ? 1 : 0))
    if (parts.length === 0) return
    this._sendSensorBatch(17, Buffer.concat(parts))
  }

  /** temperature in m°C, pressure in Pa (kPa × 1000). */
  sendEnvironmentData(temperatureE3?: number, pressureE3?: number, rain?: number): void {
    const parts: Buffer[] = []
    if (temperatureE3 !== undefined) parts.push(fieldVarint(1, temperatureE3))
    if (pressureE3 !== undefined) parts.push(fieldVarint(2, pressureE3))
    if (rain !== undefined) parts.push(fieldVarint(3, rain))
    if (parts.length === 0) return
    this._sendSensorBatch(11, Buffer.concat(parts))
  }

  /** km × 10. */
  sendOdometerData(totalKmE1: number, tripKmE1?: number): void {
    const parts: Buffer[] = [fieldVarint(1, totalKmE1)]
    if (tripKmE1 !== undefined) parts.push(fieldVarint(2, tripKmE1))
    this._sendSensorBatch(5, Buffer.concat(parts))
  }

  /** Restriction bitmask. UNRESTRICTED=0. */
  sendDrivingStatusData(status: number): void {
    this._sendSensorBatch(13, fieldVarint(1, status))
  }

  /**
   * EV battery / energy model. Sent as SensorBatch.vehicle_energy_model_data
   * (field 23) — Maps reads min_usable_capacity.watt_hours as the *current*
   * battery level (not max), per AAOS Maps decompile.
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

  /**
   * Ask the phone to emit a fresh IDR. Same VideoFocusIndication(PROJECTED,
   * unsolicited=false) aasdk sends after AV setup — phone re-primes its encoder.
   */
  requestKeyframe(): void {
    if (this._state !== State.RUNNING) return
    // payload: [focus=PROJECTED(1)]; unsolicited defaults to false
    this._sendEncrypted(
      CH.VIDEO,
      FRAME_FLAGS.ENC_SIGNAL,
      AV_MSG.VIDEO_FOCUS_INDICATION,
      Buffer.from([0x08, 0x01])
    )
    if (DEBUG) {
      console.log(
        '[Session] keyframe requested via VideoFocusIndication(PROJECTED, unsolicited=false)'
      )
    }
  }

  /**
   * HU-initiated shutdown via ByeByeRequest. Idempotent.
   * Default reason=1 (USER_SELECTION) — explicit user action in the HU shell.
   */
  requestShutdown(reason = 1 /* USER_SELECTION */): void {
    if (this._state >= State.CLOSED) return
    const payload = Buffer.from([0x08, reason & 0xff])
    if (DEBUG) console.log(`[Session] requesting shutdown reason=${reason}`)
    try {
      this._sendEncrypted(CH.CONTROL, FRAME_FLAGS.ENC_SIGNAL, CTRL_MSG.SHUTDOWN_REQUEST, payload)
    } catch (err) {
      if (DEBUG) console.warn(`[Session] shutdown send failed: ${(err as Error).message}`)
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
    // Coalesce same-tick TLS handshake chunks into one SSL_HANDSHAKE frame.
    // Electron's BoringSSL splits the 2nd flight across multiple _write()s;
    // aasdk on the phone needs the flight as one blob or it FINs with
    // "bad record mac". setImmediate flush fires once after the tick.
    const hsOutBuf: Buffer[] = []
    let hsFlushScheduled = false
    const flushHs = (): void => {
      hsFlushScheduled = false
      if (hsOutBuf.length === 0) return
      const all = Buffer.concat(hsOutBuf)
      const n = hsOutBuf.length
      hsOutBuf.length = 0
      if (DEBUG) {
        const note = n > 1 ? ` coalesced from ${n} chunks` : ''
        console.log(
          `[Session] TLS → phone: ${all.length}B (SSL_HANDSHAKE${note}): ${all.toString('hex')}`
        )
      }
      const frame = encodeFrame(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.SSL_HANDSHAKE, all)
      this._sock.write(frame)
    }

    const { tlsSocket, bridge } = createTlsClient(
      HU_CERT_PEM,
      HU_KEY_PEM,
      // Handshake: wrap in SSL_HANDSHAKE frames. Post-handshake: _pendingChannelId/Flags
      // are set in _sendAA right before tlsSocket.write(); _writeChain serialises so the
      // pending values always match this record.
      (tlsBytes) => {
        if (this._state === State.TLS_HANDSHAKE) {
          hsOutBuf.push(tlsBytes)
          if (!hsFlushScheduled) {
            hsFlushScheduled = true
            setImmediate(flushHs)
          }
        } else {
          const header = Buffer.allocUnsafe(4)
          header.writeUInt8(this._pendingChannelId, 0)
          header.writeUInt8(this._pendingFlags, 1)
          header.writeUInt16BE(tlsBytes.length, 2)
          if (DEBUG && (TRACE || !isFrameChannel(this._pendingChannelId))) {
            console.log(
              `[Session] sock→ ENC ch=${this._pendingChannelId} flags=0x${this._pendingFlags.toString(16)} ${tlsBytes.length}B`
            )
          }
          this._sock.write(Buffer.concat([header, tlsBytes]))
        }
      }
    )

    this._bridge = bridge
    this._tlsSocket = tlsSocket

    // Cleartext per decrypted AA-frame record. ctx is queued 1:1 in _stripHeaderAndInjectTls.
    // Reassembly: BULK(0x03) emit; FIRST(0x01) start; MIDDLE append; LAST(0x02) concat+emit.
    // msgId lives in the FIRST fragment's first 2 bytes only.
    tlsSocket.on('data', (chunk: Buffer) => {
      const ctx = this._channelQueue.shift()
      if (!ctx) {
        if (DEBUG)
          console.warn(`[Session] TLS data (${chunk.length}B) without channel ctx — dropping`)
        return
      }
      const isFirst = (ctx.flags & 0x01) !== 0
      const isLast = (ctx.flags & 0x02) !== 0

      // BULK — single-frame message, emit immediately.
      if (isFirst && isLast) {
        if (chunk.length < 2) {
          if (DEBUG) console.warn(`[Session] TLS decrypted payload too short (${chunk.length}B)`)
          return
        }
        const msgId = chunk.readUInt16BE(0)
        const payload = chunk.subarray(2)
        if (
          DEBUG &&
          (TRACE || (!isFrameChannel(ctx.channelId) && !isPingPong(ctx.channelId, msgId)))
        ) {
          console.log(
            `[Session] ← ch=${ctx.channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} len=${payload.length}`
          )
        }
        this._handleDecryptedMessage(ctx.channelId, ctx.flags, msgId, payload)
        return
      }

      // FIRST — start cleartext accumulator for this channel.
      if (isFirst && !isLast) {
        this._tlsCleartextFragments.set(ctx.channelId, { parts: [chunk], flags: ctx.flags })
        if (DEBUG && (TRACE || !isFrameChannel(ctx.channelId))) {
          console.log(
            `[Session] TLS cleartext frag-start ch=${ctx.channelId} have=${chunk.length}B`
          )
        }
        return
      }

      // MIDDLE / LAST — append cleartext.
      const state = this._tlsCleartextFragments.get(ctx.channelId)
      if (!state) {
        if (DEBUG) {
          console.warn(
            `[Session] ch=${ctx.channelId} cleartext continuation without first fragment — dropping`
          )
        }
        return
      }
      state.parts.push(chunk)

      if (!isLast) {
        if (DEBUG && (TRACE || !isFrameChannel(ctx.channelId))) {
          const have = state.parts.reduce((n, p) => n + p.length, 0)
          console.log(`[Session] TLS cleartext frag-cont  ch=${ctx.channelId} have=${have}B`)
        }
        return
      }

      // LAST — concat all cleartext fragments and emit.
      this._tlsCleartextFragments.delete(ctx.channelId)
      const full = Buffer.concat(state.parts)
      if (full.length < 2) {
        if (DEBUG) {
          console.warn(
            `[Session] ch=${ctx.channelId} reassembled cleartext too short (${full.length}B)`
          )
        }
        return
      }
      const msgId = full.readUInt16BE(0)
      const payload = full.subarray(2)
      if (DEBUG && (TRACE || !isFrameChannel(ctx.channelId))) {
        console.log(
          `[Session] ← ch=${ctx.channelId} msgId=0x${msgId.toString(16).padStart(4, '0')} len=${payload.length} (reassembled from ${state.parts.length} fragments)`
        )
      }
      // Use FIRST fragment's flags — only first/last bits differ across fragments.
      this._handleDecryptedMessage(ctx.channelId, state.flags, msgId, payload)
    })

    tlsSocket.on('error', (err) => {
      if (DEBUG) console.error('[Session] TLS error:', err.message)
      this.emit('error', err)
    })

    tlsSocket.on('secureConnect', () => {
      if (DEBUG) console.log('[Session] TLS handshake complete')
      this._transition(State.AUTH)
      void this._postTlsSetup()
    })

    // tls.connect() starts the handshake automatically.
  }

  private async _postTlsSetup(): Promise<void> {
    // AUTH_COMPLETE is sent PLAINTEXT (aasdk EncryptionType for ch=0/0x0004 = PLAIN).
    const authBuf = encode(this._proto.AuthCompleteIndication, { status: STATUS_OK })
    if (DEBUG) console.log(`[Session] AUTH_COMPLETE proto bytes: ${authBuf.toString('hex')}`)
    this._sendAA(CH.CONTROL, FRAME_FLAGS.PLAINTEXT, CTRL_MSG.AUTH_COMPLETE, authBuf)
    this._transition(State.SERVICE_DISCOVERY)
    if (DEBUG) console.log('[Session] AUTH_COMPLETE sent — waiting for SERVICE_DISCOVERY_REQUEST')
  }

  // ── ServiceDiscoveryResponse builder ─────────────────────────────────────

  private _buildServiceDiscoveryResponse(): Buffer {
    const cfg = this._cfg
    const vW = cfg.videoWidth ?? 1280
    const vH = cfg.videoHeight ?? 720
    const dpi = cfg.videoDpi ?? 140
    const fps = cfg.videoFps ?? 30

    // VideoCodecResolutionType: 800x480=1, 1280x720=2, 1920x1080=3, 2560x1440=4, 3840x2160=5
    const vRes: number =
      vW >= 3840
        ? 5
        : vW >= 2560
          ? 4
          : vW >= 1920
            ? VIDEO_RESOLUTION._1920x1080
            : vW <= 800
              ? VIDEO_RESOLUTION._800x480
              : VIDEO_RESOLUTION._1280x720

    const vFps = fps === 60 ? VIDEO_FPS._60 : VIDEO_FPS._30

    // Non-tier display fit: phone encodes the full tier with symmetric
    // black bars; renderer crops them off display-side.
    let widthMargin = 0
    let heightMargin = 0
    if (cfg.displayWidth && cfg.displayHeight && vW > 0 && vH > 0) {
      const displayAR = cfg.displayWidth / cfg.displayHeight
      const tierAR = vW / vH
      if (displayAR > tierAR) {
        // wider display → letterbox
        const contentH = Math.round(vW / displayAR) & ~1
        heightMargin = Math.max(0, vH - contentH)
      } else if (displayAR < tierAR) {
        // narrower display → pillarbox
        const contentW = Math.round(vH * displayAR) & ~1
        widthMargin = Math.max(0, vW - contentW)
      }
    }
    // UiConfig.margins forces symmetric split; without it the phone top-aligns.
    const insetTop = Math.floor(heightMargin / 2)
    const insetBottom = heightMargin - insetTop
    const insetLeft = Math.floor(widthMargin / 2)
    const insetRight = widthMargin - insetLeft

    // AudioStreamType: GUIDANCE=1, SYSTEM=2, MEDIA=3, TELEPHONY=4
    const AS_GUIDANCE = 1,
      AS_SYSTEM = 2,
      AS_MEDIA = 3

    // SensorType (aasdk numeric values)
    const SENSOR = {
      LOCATION: 1,
      COMPASS: 2,
      SPEED: 3,
      RPM: 4,
      ODOMETER: 5,
      FUEL: 6,
      PARKING_BRAKE: 7,
      GEAR: 8,
      NIGHT_MODE: 10,
      ENV_DATA: 11,
      HVAC: 12,
      DRIVING_STATUS: 13,
      DOOR_DATA: 16,
      LIGHT_DATA: 17,
      TIRE_PRESSURE_DATA: 18,
      ACCELEROMETER: 19,
      GYROSCOPE: 20,
      GPS_SATELLITE: 21,
      // EV / energy-routing sensors. Advertising these triggers the phone to
      // subscribe to type 23 for battery + range data. Maps' EV range display
      // depends on at least 23 + supportedFuelTypes containing ELECTRIC.
      VEHICLE_ENERGY_MODEL: 23,
      RAW_VEHICLE_ENERGY_MODEL: 25,
      RAW_EV_TRIP_SETTINGS: 26
    } as const

    const channels: object[] = []

    // ── Video (ch=3) ──
    channels.push({
      id: CH.VIDEO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.VIDEO_H264_BP,
        availableWhileInCall: true,
        videoConfigs: [
          {
            codecResolution: vRes,
            frameRate: vFps,
            widthMargin,
            heightMargin,
            density: dpi,
            videoCodecType: MEDIA_CODEC.VIDEO_H264_BP,
            uiConfig: {
              margins: { top: insetTop, bottom: insetBottom, left: insetLeft, right: insetRight }
            }
          }
        ]
      }
    })

    // ── Media Audio (ch=4) ──
    channels.push({
      id: CH.MEDIA_AUDIO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.AUDIO_PCM,
        audioType: AS_MEDIA,
        availableWhileInCall: true,
        audioConfigs: [{ samplingRate: 48000, numberOfBits: 16, numberOfChannels: 2 }]
      }
    })

    // ── Speech / Guidance Audio (ch=5) ──
    channels.push({
      id: CH.SPEECH_AUDIO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.AUDIO_PCM,
        audioType: AS_GUIDANCE,
        availableWhileInCall: true,
        audioConfigs: [{ samplingRate: 16000, numberOfBits: 16, numberOfChannels: 1 }]
      }
    })

    // ── System Audio (ch=6) ──
    channels.push({
      id: CH.SYSTEM_AUDIO,
      mediaSinkService: {
        availableType: MEDIA_CODEC.AUDIO_PCM,
        audioType: AS_SYSTEM,
        availableWhileInCall: true,
        audioConfigs: [{ samplingRate: 16000, numberOfBits: 16, numberOfChannels: 1 }]
      }
    })

    // ── Audio Input / Microphone (ch=9) ──
    channels.push({
      id: CH.MIC_INPUT,
      mediaSourceService: {
        availableType: MEDIA_CODEC.AUDIO_PCM,
        audioConfig: { samplingRate: 16000, numberOfBits: 16, numberOfChannels: 1 },
        availableWhileInCall: true
      }
    })

    // ── Sensor Source (ch=1) ──
    // FuelType: UNLEADED=1, LEADED=2, DIESEL_1=3, DIESEL_2=4, BIODIESEL=5,
    //           E85=6, LPG=7, CNG=8, LNG=9, ELECTRIC=10, HYDROGEN=11, OTHER=12
    // EV connector: J1772=1, MENNEKES=2, CHADEMO=3, COMBO_1=4, COMBO_2=5,
    //               TESLA_SUPERCHARGER=8, GBT=9, OTHER=101
    const fuelTypes = cfg.fuelTypes && cfg.fuelTypes.length > 0 ? cfg.fuelTypes : [1]
    const evConnectorTypes = cfg.evConnectorTypes ?? []

    channels.push({
      id: CH.SENSOR,
      sensorSourceService: {
        sensors: [
          { sensorType: SENSOR.DRIVING_STATUS },
          { sensorType: SENSOR.LOCATION },
          { sensorType: SENSOR.NIGHT_MODE },
          { sensorType: SENSOR.SPEED },
          { sensorType: SENSOR.GEAR },
          { sensorType: SENSOR.PARKING_BRAKE },
          { sensorType: SENSOR.FUEL },
          { sensorType: SENSOR.ODOMETER },
          { sensorType: SENSOR.ENV_DATA },
          { sensorType: SENSOR.DOOR_DATA },
          { sensorType: SENSOR.LIGHT_DATA },
          { sensorType: SENSOR.TIRE_PRESSURE_DATA },
          { sensorType: SENSOR.HVAC },
          { sensorType: SENSOR.ACCELEROMETER },
          { sensorType: SENSOR.GYROSCOPE },
          { sensorType: SENSOR.COMPASS },
          { sensorType: SENSOR.GPS_SATELLITE },
          { sensorType: SENSOR.RPM },
          // EV energy model — triggers phone to request battery routing data
          { sensorType: SENSOR.VEHICLE_ENERGY_MODEL },
          { sensorType: SENSOR.RAW_VEHICLE_ENERGY_MODEL },
          { sensorType: SENSOR.RAW_EV_TRIP_SETTINGS }
        ],
        // RAW_GPS_ONLY=256 | ACCEL=4 | GYRO=2 | COMPASS=8 | CAR_SPEED=64
        locationCharacterization: 256 | 4 | 2 | 8 | 64,
        supportedFuelTypes: fuelTypes,
        ...(evConnectorTypes.length > 0 ? { supportedEvConnectorTypes: evConnectorTypes } : {})
      }
    })

    // ── Input Source (ch=8) ──
    // Touch dims match the AA tier, not the physical display.
    let touchW = 1920,
      touchH = 1080
    switch (vRes) {
      case 1:
        touchW = 800
        touchH = 480
        break
      case 2:
        touchW = 1280
        touchH = 720
        break
      case 3:
        touchW = 1920
        touchH = 1080
        break
      case 4:
        touchW = 2560
        touchH = 1440
        break
      case 5:
        touchW = 3840
        touchH = 2160
        break
    }
    void vH
    channels.push({
      id: CH.INPUT,
      inputSourceService: {
        // KeyEvent.KEYCODE_* + AA extensions — mirrors BUTTON_KEY in InputChannel.ts.
        keycodesSupported: [
          // System
          3, // HOME
          4, // BACK
          5,
          6, // CALL, ENDCALL
          7,
          8,
          9,
          10,
          11,
          12,
          13,
          14,
          15,
          16, // 0-9 (DTMF)
          17,
          18, // STAR, POUND
          // D-PAD: LEFT/RIGHT (21/22) for top-level tile cycling (interim
          // up/down → tile-switch mapping in aaDriver) and CENTER (23) for
          // select. UP/DOWN (19/20) intentionally omitted — vertical nav is
          // driven via ROTARY_CONTROLLER (65536) RelativeEvent.delta.
          21,
          22,
          23, // DPAD_LEFT, DPAD_RIGHT, DPAD_CENTER
          24,
          25, // VOLUME +/− (audioTransferMode)
          26,
          66,
          79,
          82,
          84, // POWER, ENTER, HEADSET_HOOK, MENU, SEARCH
          85,
          86,
          87,
          88,
          89,
          90,
          91,
          111,
          126,
          127,
          164, // media transport + mute
          219,
          231, // VOICE_ASSIST, CALL_VOICE
          260,
          261,
          262,
          263, // NAVIGATE
          65536 // KEYCODE_ROTARY_CONTROLLER
        ],
        touchscreen: [{ width: touchW, height: touchH }]
      }
    })

    // ── Bluetooth (ch=10) ──
    channels.push({
      id: CH.BLUETOOTH,
      bluetoothService: {
        carAddress: cfg.btMacAddress ?? '00:00:00:00:00:00',
        supportedPairingMethods: [BT_PAIRING_METHOD.PIN, BT_PAIRING_METHOD.NUMERIC_COMPARISON]
      }
    })

    // ── Navigation Status (ch=12) ──
    channels.push({
      id: CH.NAVIGATION,
      navigationStatusService: {
        minimumIntervalMs: 500,
        type: 1, // IMAGE
        imageOptions: { width: 256, height: 256, colourDepthBits: 32 }
      }
    })

    // ── Media Playback Status (ch=13) — empty body, presence-only ──
    channels.push({ id: CH.MEDIA_INFO, mediaPlaybackService: {} })

    // ── Phone Status (ch=14) ──
    channels.push({ id: CH.PHONE_STATUS, phoneStatusService: {} })

    // WiFi Projection (ch=18)
    if (cfg.wifiBssid) {
      channels.push({
        id: CH.WIFI,
        wifiProjectionService: { carWifiBssid: cfg.wifiBssid }
      })
    }

    const sdrFields: Record<string, unknown> = {
      driverPosition: cfg.driverPosition ?? 0, // LHD=0 (LEFT), RHD=1 (RIGHT)
      displayName: cfg.huName ?? 'LIVI',
      probeForSupport: false,
      connectionConfiguration: {
        pingConfiguration: {
          timeoutMs: 5000,
          intervalMs: 1500,
          highLatencyThresholdMs: 500,
          trackedPingCount: 5
        }
      },
      headunitInfo: {
        make: 'LIVI',
        model: 'Universal',
        year: '2026',
        vehicleId: 'livi-001',
        headUnitMake: 'LIVI',
        headUnitModel: 'LIVI Head Unit',
        headUnitSoftwareBuild: '1',
        headUnitSoftwareVersion: '1.0'
      },
      make: 'LIVI',
      model: 'Universal',
      year: '2026',
      vehicleId: 'livi-001',
      headUnitMake: 'LIVI',
      headUnitModel: 'LIVI Head Unit',
      headUnitSoftwareBuild: '1',
      headUnitSoftwareVersion: '1.0',
      canPlayNativeMediaDuringVr: true,
      channels
    }

    const msg = this._proto.ServiceDiscoveryResponse.create(sdrFields)
    const buf = Buffer.from(this._proto.ServiceDiscoveryResponse.encode(msg).finish())

    if (DEBUG) {
      console.log(`[Session] SDR (aasdk aap_protobuf): ${channels.length} channels, ${buf.length}B`)
      console.log(
        `[Session] SDR hex: ${buf.subarray(0, 64).toString('hex')}${buf.length > 64 ? '...' : ''}`
      )
    }
    return buf
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
    // max_unacked=1: phone paces to real-time. Higher values cause burst+stall.
    const respBuf = encode(this._proto.AVChannelSetupResponse, {
      mediaStatus: AV_SETUP_STATUS.OK,
      maxUnacked: 1,
      configs: [0]
    })
    this._sendEncrypted(channelId, FRAME_FLAGS.ENC_SIGNAL, AV_MSG.SETUP_RESPONSE, respBuf)
    if (DEBUG) {
      console.log(
        `[Session] AVChannelSetupResponse ch=${channelId} status=OK(${AV_SETUP_STATUS.OK}) sent`
      )
    }

    if (channelId === CH.VIDEO) {
      // VideoFocusIndication(PROJECTED, unsolicited=false) IS the keyframe-request
      // mechanism in aasdk — triggers a fresh IDR. unsolicited=true would stall.
      this._sendEncrypted(
        CH.VIDEO,
        FRAME_FLAGS.ENC_SIGNAL,
        AV_MSG.VIDEO_FOCUS_INDICATION,
        Buffer.from([0x08, 0x01])
      )
      if (DEBUG) {
        console.log(
          '[Session] VideoFocusIndication (PROJECTED, unsolicited=false) sent — requests fresh IDR'
        )
      }

      // No AVChannelStartIndication — phone sends START_INDICATION when ready.
      this._transition(State.RUNNING)
      this.emit('connected')
      if (DEBUG) console.log('[Session] Video channel ready — waiting for H.264 frames from phone')
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
      // NightMode = false
      this._sendEncrypted(
        CH.SENSOR,
        FRAME_FLAGS.ENC_SIGNAL,
        0x8003,
        Buffer.from([0x52, 0x02, 0x08, 0x00])
      )
      if (DEBUG) console.log('[Session] SensorBatch: NightMode=false sent')
    } else {
      if (DEBUG) {
        console.log(`[Session] SensorStartRequest type=${sensorType} — no batch data for this type`)
      }
    }
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
    //
    // STATIC at f5 is what makes the phone persist the WifiConfiguration —
    // DYNAMIC is treated as a transient hotspot and discarded.
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

    parts.push(Buffer.from([0x10, 0x05])) // security_mode = WPA2_PERSONAL (new enum)

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

  /**
   * Send an AA frame. Encrypted (flags & 0x08) → TLS via tlsSocket. Plaintext
   * → raw on TCP (AUTH_COMPLETE / PING are always plaintext per aasdk).
   */
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

    if (!this._tlsSocket || this._state < State.AUTH) {
      if (DEBUG) console.warn('[Session] _sendAA: TLS not ready for encrypted frame')
      return
    }

    const msgIdBuf = Buffer.allocUnsafe(2)
    msgIdBuf.writeUInt16BE(msgId, 0)
    const cleartext = Buffer.concat([msgIdBuf, data])

    // Serialise — Node's _writev would otherwise merge consecutive writes into
    // one TLS record, and the bridge would tag it with the last channelId/flags,
    // dropping earlier messages' headers.
    const sock = this._tlsSocket
    this._writeChain = this._writeChain.then(
      () =>
        new Promise<void>((resolve) => {
          this._pendingChannelId = channelId
          this._pendingFlags = flags
          sock.write(cleartext, () => resolve())
        })
    )
    this._writeChain.catch((err) => {
      if (DEBUG) console.warn('[Session] _writeChain rejected:', err)
    })
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
